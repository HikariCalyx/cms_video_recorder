// recorder/backend.rs – Windows.Graphics.Capture + Media Foundation backend

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
    VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

// Win32 geometry comes from our own `windows` dependency rather than the one
// `windows-capture` pulls in; only the raw HWND value crosses between them.
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

use super::audio::{AudioCapture, AudioFormat};
use super::{Error, RecorderConfig};

// ---------------------------------------------------------------------------
// Client-area geometry
// ---------------------------------------------------------------------------

/// How a capture frame maps onto encoder input.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Region of the capture frame that gets encoded.
    src_x: u32,
    src_y: u32,
    /// Source region size: either the encoded size, or twice it when halving.
    src_width: u32,
    src_height: u32,
    /// Encoded frame size. Always even, as H.264 requires.
    out_width: u32,
    out_height: u32,
}

impl Layout {
    /// Whether 2x2 source blocks are averaged down to one output pixel.
    fn halves(&self) -> bool {
        self.out_width != self.src_width
    }

}

/// Decides the source region and encoded size for a capture frame.
fn plan_layout(frame_width: u32, frame_height: u32, config: &RecorderConfig, hwnd: isize) -> Layout {
    let (src_x, src_y, width, height) = config
        .exclude_window_border
        .then(|| client_rect(hwnd, frame_width, frame_height))
        .flatten()
        .unwrap_or((0, 0, frame_width, frame_height));

    // Oversized captures are halved so a 1440p or 4K game window lands at a
    // sane file size, and so the fixed bitrate stays appropriate.
    let halve = config
        .max_native_size
        .is_some_and(|(max_w, max_h)| width > max_w || height > max_h);

    if halve {
        // Consume exactly twice what is emitted, so every output pixel is a
        // full 2x2 average and no partial block is left over. The game's
        // resolutions all divide evenly here.
        let out_width = (width / 2) & !1;
        let out_height = (height / 2) & !1;

        Layout {
            src_x,
            src_y,
            src_width: out_width * 2,
            src_height: out_height * 2,
            out_width,
            out_height,
        }
    } else {
        let out_width = width & !1;
        let out_height = height & !1;

        Layout {
            src_x,
            src_y,
            src_width: out_width,
            src_height: out_height,
            out_width,
            out_height,
        }
    }
}

/// Size of the frames WGC will hand out for `hwnd`.
///
/// A window capture covers the DWM extended frame bounds, so that size is also
/// the capture surface size — verified against live captures. Knowing it before
/// the first frame arrives is what lets the encoder be built up front.
///
/// Falls back to the window rect, which is larger by the invisible resize
/// border; the encoder pads or trims to its target size, so a stale guess
/// degrades the framing rather than breaking the recording.
fn capture_frame_size(hwnd: isize) -> (u32, u32) {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    let mut bounds = RECT::default();

    let measured = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut bounds as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok()
    };

    if !measured {
        unsafe {
            let _ = GetWindowRect(hwnd, &mut bounds);
        }
    }

    (
        (bounds.right - bounds.left).max(0) as u32,
        (bounds.bottom - bounds.top).max(0) as u32,
    )
}

/// Locates the client area inside the frame WGC hands out for `hwnd`.
///
/// A window capture covers the DWM extended frame bounds: the visible window
/// including its title bar and 1px borders, but excluding the invisible resize
/// margin. The client area is the game's viewport, so the offset between the
/// two is exactly the chrome to strip — measured at 31px on top and 1px on the
/// other three sides for a standard `WS_OVERLAPPEDWINDOW` at 96 DPI.
///
/// Returned as `(x, y, width, height)`, or `None` when the geometry can't be
/// trusted and the full frame should be used instead.
fn client_rect(hwnd: isize, frame_width: u32, frame_height: u32) -> Option<(u32, u32, u32, u32)> {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);

    let mut bounds = RECT::default();
    let mut client = RECT::default();
    let mut origin = POINT::default();

    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut bounds as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        GetClientRect(hwnd, &mut client).ok()?;
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
    }

    // The reported bounds have to match the actual capture surface, otherwise
    // the offsets below are in the wrong coordinate space – which is what
    // happens when the target process is being DPI-virtualised. Fall back to
    // the uncropped frame instead of cropping blindly.
    if bounds.right - bounds.left != frame_width as i32
        || bounds.bottom - bounds.top != frame_height as i32
    {
        return None;
    }

    let x = (origin.x - bounds.left).max(0) as u32;
    let y = (origin.y - bounds.top).max(0) as u32;

    let width = (client.right.max(0) as u32).min(frame_width.saturating_sub(x));
    let height = (client.bottom.max(0) as u32).min(frame_height.saturating_sub(y));

    if width == 0 || height == 0 {
        return None;
    }

    Some((x, y, width, height))
}

/// Repacks `frame` into `dst` as the encoder wants it: cropped to the client
/// area, optionally halved, bottom row first.
///
/// Media Foundation reads uncompressed BGRA samples bottom-up, so rows go out
/// in reverse. Crop, flip and downscale all happen in this one pass over the
/// mapped staging texture, so the extra work rides along with the copy this
/// path already needs.
fn repack(frame: &mut Frame, layout: Layout, dst: &mut Vec<u8>) -> Result<(), Error> {
    let mut buffer = frame
        .buffer_crop(
            layout.src_x,
            layout.src_y,
            layout.src_x + layout.src_width,
            layout.src_y + layout.src_height,
        )
        .map_err(|error| Error::Capture(error.to_string()))?;

    let pitch = buffer.row_pitch() as usize;
    let out_width = layout.out_width as usize;
    let out_row = out_width * 4;
    let halves = layout.halves();

    // A no-op once the first frame has sized it, so this doesn't reallocate.
    dst.resize(out_row * layout.out_height as usize, 0);

    let src = buffer.as_raw_buffer();

    // `.rev()` is the vertical flip: source row 0 is the top of the picture and
    // lands in the last row of the buffer.
    for (y, out) in dst.chunks_exact_mut(out_row).rev().enumerate() {
        if halves {
            let top = &src[(2 * y) * pitch..][..out_width * 8];
            let bottom = &src[(2 * y + 1) * pitch..][..out_width * 8];

            for (x, pixel) in out.chunks_exact_mut(4).enumerate() {
                let (t, b) = (&top[x * 8..x * 8 + 8], &bottom[x * 8..x * 8 + 8]);

                // Average the 2x2 block channel by channel. Rounding by +2
                // keeps flat areas from drifting darker.
                for (c, channel) in pixel.iter_mut().enumerate() {
                    *channel = ((u32::from(t[c])
                        + u32::from(t[c + 4])
                        + u32::from(b[c])
                        + u32::from(b[c + 4])
                        + 2)
                        / 4) as u8;
                }
            }
        } else {
            out.copy_from_slice(&src[y * pitch..][..out_row]);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Handle held by the UI
// ---------------------------------------------------------------------------

/// Whether `GraphicsCaptureSession.IsBorderRequired` can be set.
///
/// The property only exists on Windows 11 (build 22000) and later. On
/// Windows 10 the system always draws the yellow "being captured" border,
/// and windows-capture fails the whole session when `WithoutBorder` is
/// requested there, so the caller falls back to `DrawBorderSettings::Default`
/// and the client-area crop keeps the border out of the recording.
fn border_toggle_supported() -> bool {
    use windows::core::HSTRING;
    use windows::Foundation::Metadata::ApiInformation;

    ApiInformation::IsPropertyPresent(
        &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
        &HSTRING::from("IsBorderRequired"),
    )
    .unwrap_or(false)
}

/// A capture session in progress.
pub struct Recording {
    control: CaptureControl<Session, Error>,
    target: Window,
    output_path: PathBuf,
    /// Kept alive for the length of the recording; dropping it stops the
    /// loopback thread.
    audio: Option<Arc<AudioCapture>>,
    started: Instant,
}

impl Recording {
    /// Starts capturing `hwnd` into a new MP4 under `config.output_dir`.
    ///
    /// Returns as soon as the capture thread is up; encoding happens in the
    /// background.
    pub fn start(hwnd: isize, alias: &str, config: &RecorderConfig) -> Result<Self, Error> {
        // Create the directory up front rather than on the capture thread, so
        // a bad output path surfaces as an error the user sees immediately.
        std::fs::create_dir_all(&config.output_dir)?;

        let output_path = super::output_path(&config.output_dir, alias);
        let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);

        if !window.is_valid() {
            return Err(Error::Capture(crate::i18n::tr("error-target-closed")));
        }

        // Opened first: the encoder's audio stream has to be built for the
        // endpoint's sample rate. A failure here is not fatal – losing sound is
        // much better than losing the clip.
        let audio = config
            .capture_audio
            .then(AudioCapture::start)
            .and_then(|result| match result {
                Ok(capture) => Some(Arc::new(capture)),
                Err(error) => {
                    // Never shown in the UI: recording falls back to silent
                    // video, so debug builds log the reason to the console.
                    #[cfg(debug_assertions)]
                    eprintln!("[error] {error}");
                    None
                }
            });

        // The yellow "being captured" border has to be turned off where the
        // API allows it. `IsBorderRequired` only exists on Windows 11, and
        // Windows 10 rejects the whole session when asked to toggle it, so
        // there the border is left to the system default.
        let draw_border = if border_toggle_supported() {
            DrawBorderSettings::WithoutBorder
        } else {
            DrawBorderSettings::Default
        };

        let settings = Settings::new(
            window,
            // The cursor is outside the game window most of the time and only
            // adds a distraction when it isn't.
            CursorCaptureSettings::WithoutCursor,
            draw_border,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            // Bgra8 matches what the compositor hands out, so no conversion
            // pass is needed before the encoder.
            ColorFormat::Bgra8,
            Flags {
                hwnd,
                output_path: output_path.clone(),
                config: config.clone(),
                audio: audio.clone(),
            },
        );

        let control = Session::start_free_threaded(settings)
            .map_err(|error| Error::Capture(error.to_string()))?;

        // Sit out the warm-up the capture thread is discarding, so the caller's
        // clock starts on the first frame that actually reaches the file. The
        // countdown then matches the recording's real length.
        std::thread::sleep(config.warmup);

        Ok(Self {
            control,
            target: window,
            output_path,
            audio,
            started: Instant::now(),
        })
    }

    /// How long the session has been running.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Raw handle of the window being recorded.
    pub fn target_hwnd(&self) -> isize {
        self.target.as_raw_hwnd() as isize
    }

    /// Whether the window being recorded still exists.
    ///
    /// The capture side closes the file on its own when the target goes away,
    /// but the UI has no way to hear about that, so it polls instead of
    /// counting down against a recording that already ended.
    pub fn target_is_alive(&self) -> bool {
        self.target.is_valid()
    }

    /// Stops the capture and finalises the MP4, returning its path.
    ///
    /// Blocks until the encoder has flushed and written the container index,
    /// which is what makes the file playable, so callers should run this off
    /// the UI thread.
    pub fn finish(self) -> Result<PathBuf, Error> {
        // Grab a handle to the session before `stop` consumes the control:
        // stopping only tears down the capture thread, it does not close out
        // the encoder, so we still have to finalise afterwards.
        let session = self.control.callback();

        let stop_result = self
            .control
            .stop()
            .map_err(|error| Error::Capture(error.to_string()));

        // Stop loopback before the final drain, so the audio tail doesn't keep
        // growing past where the video ended.
        if let Some(audio) = &self.audio {
            audio.stop();
        }

        // Finalise even if stopping reported a problem – a partially written
        // file is worth more than none, and the encoder may well be fine.
        let finalize_result = session.lock().close();

        stop_result.and(finalize_result).map(|()| self.output_path)
    }
}

// ---------------------------------------------------------------------------
// Capture handler
// ---------------------------------------------------------------------------

/// Audio stream settings, matched to whatever the endpoint reports.
///
/// The encoder blocks its pipeline waiting for audio samples once the stream is
/// enabled, so it stays disabled unless capture is actually running.
fn audio_settings(format: Option<AudioFormat>) -> AudioSettingsBuilder {
    match format {
        Some(format) => AudioSettingsBuilder::default()
            .sample_rate(format.sample_rate)
            .channel_count(format.channels)
            .bit_per_sample(format.bits_per_sample),
        None => AudioSettingsBuilder::default().disabled(true),
    }
}

/// Settings handed to the capture thread through `Settings::flags`.
struct Flags {
    /// Target window, needed to work out where its client area sits.
    hwnd: isize,
    output_path: PathBuf,
    config: RecorderConfig,
    /// Running loopback capture, or `None` to record silent video.
    ///
    /// Opened before the encoder because the encoder's audio stream has to be
    /// configured for whatever format the endpoint reports.
    audio: Option<Arc<AudioCapture>>,
}

/// Lives on the capture thread and owns the encoder.
struct Session {
    /// Taken by `finalize`, so `None` means the file is already closed out.
    encoder: Option<VideoEncoder>,
    /// Frames arriving before this are dropped while the pipeline settles.
    /// Set from the first frame, so the clock tracks real capture activity
    /// rather than however long the encoder took to build.
    keep_from: Option<Instant>,
    /// Arrival of the first kept frame – the clock the duration cap runs on.
    first_frame: Option<Instant>,
    /// Source region and encoded size, fixed before capture starts.
    layout: Layout,
    /// Reused destination for repacked pixels, so the copy doesn't allocate.
    scratch: Vec<u8>,
    flags: Flags,
}

impl Session {
    /// Flushes the last of the audio, then closes the file out.
    fn close(&mut self) -> Result<(), Error> {
        if self.encoder.is_some() {
            // Best effort: a failed flush shouldn't stop the file being closed,
            // or the whole recording would be lost over a few ms of sound.
            let _ = self.pump_audio();
        }

        self.finalize()
    }

    /// Flushes the encoder and writes the MP4 container index.
    ///
    /// Idempotent, because both the duration cap and the UI can trigger it.
    fn finalize(&mut self) -> Result<(), Error> {
        match self.encoder.take() {
            Some(encoder) => encoder
                .finish()
                .map_err(|error| Error::Encoder(error.to_string())),
            None => Ok(()),
        }
    }

    /// Hands whatever loopback has captured to the encoder.
    ///
    /// Driven from frame arrival rather than its own thread so the encoder is
    /// only ever touched from the capture thread. At 60fps that's a handful of
    /// milliseconds of audio per call, well inside the encoder's buffer.
    fn pump_audio(&mut self) -> Result<(), Error> {
        let Some(audio) = &self.flags.audio else {
            return Ok(());
        };
        let Some(pcm) = audio.take_pcm() else {
            return Ok(());
        };

        self.encoder
            .as_mut()
            .expect("caller checked the encoder is open")
            .send_audio_buffer(&pcm, 0)
            .map_err(|error| Error::Encoder(error.to_string()))
    }

    /// Builds the encoder.
    ///
    /// Called before the capture session starts, not on the first frame:
    /// spinning up Media Foundation takes a few hundred milliseconds, and doing
    /// it inside `on_frame_arrived` stalled frame delivery for that long. The
    /// gap left a run of duplicate frames at the head of every recording.
    fn create_encoder(flags: &Flags, layout: Layout) -> Result<VideoEncoder, Error> {
        let (width, height) = (layout.out_width, layout.out_height);

        if width == 0 || height == 0 {
            return Err(Error::Capture(crate::i18n::tr("error-target-size")));
        }

        VideoEncoder::new(
            VideoSettingsBuilder::new(width, height)
                // H.264 in place of the default HEVC: it plays everywhere
                // without a codec install, which matters for clips that get
                // shared around.
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(flags.config.frame_rate)
                .bitrate(flags.config.bitrate),
            audio_settings(flags.audio.as_deref().map(|audio| audio.format())),
            // MPEG4 is the default container, i.e. the .mp4 we want.
            ContainerSettingsBuilder::default(),
            &flags.output_path,
        )
        .map_err(|error| Error::Encoder(error.to_string()))
    }
}

impl GraphicsCaptureApiHandler for Session {
    type Flags = Flags;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let flags = ctx.flags;

        // Runs before the capture session starts, so the encoder is ready by
        // the time the first frame shows up.
        let (frame_width, frame_height) = capture_frame_size(flags.hwnd);
        let layout = plan_layout(frame_width, frame_height, &flags.config, flags.hwnd);
        let encoder = Self::create_encoder(&flags, layout)?;

        Ok(Self {
            encoder: Some(encoder),
            keep_from: None,
            first_frame: None,
            layout,
            scratch: Vec::new(),
            flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.encoder.is_none() {
            // Already finalised – the session is winding down, so drop the
            // frame instead of reopening the file.
            capture_control.stop();
            return Ok(());
        }

        // Let the pipeline settle before keeping anything, otherwise the file
        // opens on a run of duplicate frames.
        let warmup = self.flags.config.warmup;
        let keep_from = *self
            .keep_from
            .get_or_insert_with(|| Instant::now() + warmup);

        if Instant::now() < keep_from {
            return Ok(());
        }

        let first = self.first_frame.is_none();
        let start = *self.first_frame.get_or_insert_with(Instant::now);

        if first {
            // Both streams start counting from here, which is what keeps them
            // in sync: the encoder times audio purely by samples received.
            if let Some(audio) = &self.flags.audio {
                audio.arm();
            }
        }

        let layout = self.layout;

        // Resizing the window mid-recording can shrink the capture surface
        // below the region fixed when the encoder was built. Rather than read
        // out of bounds, those frames go straight to the encoder, which pads
        // them to the target size – the picture shifts, but recording survives.
        let fits = layout.src_x + layout.src_width <= frame.width()
            && layout.src_y + layout.src_height <= frame.height();

        if !fits {
            // Only reachable while the window is being resized. Hand the
            // surface over as-is and let the encoder pad it.
            self.encoder
                .as_mut()
                .expect("checked above")
                .send_frame(frame)
                .map_err(|error| Error::Encoder(error.to_string()))?;
        } else {
            // Always repack, even when no crop or scale is needed.
            //
            // `send_frame` queues the capture surface by reference, and the
            // frame pool only has one buffer, so the compositor overwrites it
            // before the encoder gets around to reading it. That aliasing
            // showed up as long runs of duplicate frames. Copying the pixels
            // out here is what makes each frame independent.
            let timestamp = frame
                .timestamp()
                .map_err(|error| Error::Capture(error.to_string()))?
                .Duration;

            repack(frame, layout, &mut self.scratch)?;

            self.encoder
                .as_mut()
                .expect("checked above")
                .send_frame_buffer(&self.scratch, timestamp)
                .map_err(|error| Error::Encoder(error.to_string()))?;
        }

        self.pump_audio()?;

        // Enforce the cap here as well as in the UI. The UI stop is what the
        // user sees, but closing the file on this side guarantees the
        // recording never runs long even if the UI is busy or wedged.
        if self
            .flags
            .config
            .max_duration
            .is_some_and(|max| start.elapsed() >= max)
        {
            self.close()?;
            capture_control.stop();
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The target window went away mid-recording. Close out the file so
        // everything captured up to that point stays playable.
        self.finalize()
    }
}
