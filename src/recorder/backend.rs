// recorder/backend.rs – Windows.Graphics.Capture + Media Foundation backend

use std::path::PathBuf;
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

use super::{Error, RecorderConfig};

// ---------------------------------------------------------------------------
// Handle held by the UI
// ---------------------------------------------------------------------------

/// A capture session in progress.
pub struct Recording {
    control: CaptureControl<Session, Error>,
    target: Window,
    output_path: PathBuf,
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
            return Err(Error::Capture("目标窗口已关闭".to_string()));
        }

        let settings = Settings::new(
            window,
            // The cursor is outside the game window most of the time and only
            // adds a distraction when it isn't.
            CursorCaptureSettings::WithoutCursor,
            // The yellow "being captured" border would be baked into the
            // recording.
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            // Bgra8 matches what the compositor hands out, so no conversion
            // pass is needed before the encoder.
            ColorFormat::Bgra8,
            Flags {
                output_path: output_path.clone(),
                max_duration: config.max_duration,
                frame_rate: config.frame_rate,
                bitrate: config.bitrate,
            },
        );

        let control = Session::start_free_threaded(settings)
            .map_err(|error| Error::Capture(error.to_string()))?;

        Ok(Self {
            control,
            target: window,
            output_path,
            started: Instant::now(),
        })
    }

    /// How long the session has been running.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
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

        // Finalise even if stopping reported a problem – a partially written
        // file is worth more than none, and the encoder may well be fine.
        let finalize_result = session.lock().finalize();

        stop_result.and(finalize_result).map(|()| self.output_path)
    }
}

// ---------------------------------------------------------------------------
// Capture handler
// ---------------------------------------------------------------------------

/// Settings handed to the capture thread through `Settings::flags`.
struct Flags {
    output_path: PathBuf,
    max_duration: Duration,
    frame_rate: u32,
    bitrate: u32,
}

/// Lives on the capture thread and owns the encoder.
struct Session {
    /// Created on the first frame, then taken by `finalize`.
    ///
    /// `None` means either no frame has arrived yet or the file has already
    /// been closed out.
    encoder: Option<VideoEncoder>,
    /// Set to `true` once the first frame has been seen, so `finalize` can
    /// tell "nothing was ever captured" apart from "already finalised".
    started: bool,
    /// Timestamp of the first frame – the clock the duration cap runs on.
    first_frame: Option<Instant>,
    flags: Flags,
}

impl Session {
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

    /// Builds the encoder for the frame geometry the compositor actually gave
    /// us.
    ///
    /// Deferring this to the first frame avoids guessing at the capture size
    /// from the window rect, which is off by the invisible resize border and
    /// gets rounded by the compositor.
    fn create_encoder(&self, frame: &Frame) -> Result<VideoEncoder, Error> {
        // H.264 requires even dimensions. Frames are padded or cropped to the
        // encoder's size, so rounding down loses at most one row or column.
        let width = frame.width() & !1;
        let height = frame.height() & !1;

        if width == 0 || height == 0 {
            return Err(Error::Capture("目标窗口尺寸无效".to_string()));
        }

        VideoEncoder::new(
            VideoSettingsBuilder::new(width, height)
                // H.264 in place of the default HEVC: it plays everywhere
                // without a codec install, which matters for clips that get
                // shared around.
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(self.flags.frame_rate)
                .bitrate(self.flags.bitrate),
            AudioSettingsBuilder::default().disabled(true),
            // MPEG4 is the default container, i.e. the .mp4 we want.
            ContainerSettingsBuilder::default(),
            &self.flags.output_path,
        )
        .map_err(|error| Error::Encoder(error.to_string()))
    }
}

impl GraphicsCaptureApiHandler for Session {
    type Flags = Flags;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            encoder: None,
            started: false,
            first_frame: None,
            flags: ctx.flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.started && self.encoder.is_none() {
            // Already finalised – the session is winding down, so drop the
            // frame instead of reopening the file.
            capture_control.stop();
            return Ok(());
        }

        if self.encoder.is_none() {
            self.encoder = Some(self.create_encoder(frame)?);
            self.started = true;
        }

        let start = *self.first_frame.get_or_insert_with(Instant::now);

        self.encoder
            .as_mut()
            .expect("encoder was just created")
            .send_frame(frame)
            .map_err(|error| Error::Encoder(error.to_string()))?;

        // Enforce the cap here as well as in the UI. The UI stop is what the
        // user sees, but closing the file on this side guarantees the
        // recording never runs long even if the UI is busy or wedged.
        if start.elapsed() >= self.flags.max_duration {
            self.finalize()?;
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
