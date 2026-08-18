// trim.rs – clip trimming before compression
//
// The trim panel previews the selected range of a clip: ffmpeg decodes the
// segment into raw RGBA frames, paced in real time with `-re`, and the UI
// drains the frame pipe through a tick subscription. The same module reads
// the clip duration and resolution from ffmpeg's own banner, so no ffprobe
// binary has to ship next to ffmpeg.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::i18n::{tr, tr_arg};

/// Size of the preview box. Decoded frames are scaled to fit inside it,
/// preserving the source aspect ratio (the box itself letterboxes them).
pub const PREVIEW_MAX_WIDTH: u32 = 436;
pub const PREVIEW_MAX_HEIGHT: u32 = 240;

/// Timeline geometry. The window is a fixed 460 px wide and the panel pads
/// 12 px on each side, so the timeline is always exactly this wide.
pub const TIMELINE_WIDTH: f32 = 436.0;
pub const TIMELINE_HEIGHT: f32 = 30.0;
pub const KNOB_WIDTH: f32 = 12.0;
/// Shortest selectable segment, in seconds.
pub const MIN_SEGMENT: f64 = 0.1;

/// What the banner probe learned about the clip.
#[derive(Debug, Clone, Copy)]
pub struct ProbeInfo {
    /// Total clip duration, in seconds.
    pub duration: f64,
    /// Natural video size, parsed from the stream banner.
    pub width: u32,
    pub height: u32,
}

impl ProbeInfo {
    /// The size frames are decoded to: the source scaled to fit the preview
    /// box, preserving the aspect ratio.
    pub fn preview_size(&self) -> (u32, u32) {
        fit(self.width, self.height, PREVIEW_MAX_WIDTH, PREVIEW_MAX_HEIGHT)
    }
}

/// Scales `width` x `height` down (or up) to fit inside `max_width` x
/// `max_height`, keeping the aspect ratio.
fn fit(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (max_width, max_height);
    }

    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
    let out_width = ((width as f64 * scale).round() as u32).max(1);
    let out_height = ((height as f64 * scale).round() as u32).max(1);
    (out_width, out_height)
}

/// One decoded frame of the preview, ready for an image handle.
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// What the preview worker reports through the pipe.
pub enum PreviewEvent {
    Frame(PreviewFrame),
    /// The segment played to its end.
    Ended,
    /// Decoding broke down; the string is the plain OS error.
    Failed(String),
}

/// Shared handles between the UI thread and the preview worker.
pub struct PreviewProcess {
    /// Bounded pipe of decoded frames, drained by the UI clock.
    pub rx: Arc<Mutex<mpsc::Receiver<PreviewEvent>>>,
    /// Live ffmpeg process, killed when the preview is stopped.
    pub child: Arc<Mutex<Option<std::process::Child>>>,
}

/// Which of the two timeline handles is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimHandle {
    Start,
    End,
}

/// State of the trim panel.
#[derive(Default)]
pub struct TrimState {
    pub is_open: bool,
    /// Clip being trimmed.
    pub source: Option<PathBuf>,
    /// Total clip duration in seconds; 0 until probed.
    pub duration: f64,
    /// Decode size for the preview, once the clip has been probed.
    pub video_size: Option<(u32, u32)>,
    /// Selected segment, in seconds from the start of the clip.
    pub start: f64,
    pub end: f64,
    /// Preview position, for the playhead marker.
    pub playhead: f64,
    pub playing: bool,
    /// When playback started, to advance the playhead in real time.
    pub play_started: Option<Instant>,
    /// Handle currently being dragged, if any.
    pub dragging: Option<TrimHandle>,
    /// Latest decoded frame.
    pub frame: Option<PreviewFrame>,
    /// The running preview, while playing.
    pub preview: Option<PreviewProcess>,
    /// Shown inside the preview box (probe failure, decode failure).
    pub error: Option<String>,
}

impl TrimState {
    /// Arms the panel for `path` and resets every field.
    pub fn open(&mut self, path: PathBuf) {
        self.stop_preview();

        self.source = Some(path);
        self.is_open = true;
        self.duration = 0.0;
        self.video_size = None;
        self.start = 0.0;
        self.end = 0.0;
        self.playhead = 0.0;
        self.playing = false;
        self.play_started = None;
        self.dragging = None;
        self.frame = None;
        self.error = None;
    }

    /// Starts decoding the selected segment in real time.
    pub fn start_preview(&mut self) -> Result<(), String> {
        let Some(path) = self.source.clone() else {
            return Ok(());
        };
        let Some((width, height)) = self.video_size else {
            return Err(tr("trim-no-duration"));
        };
        if self.duration <= 0.0 || self.end - self.start < MIN_SEGMENT {
            return Err(tr("trim-no-duration"));
        }

        self.stop_preview();

        let preview = spawn_preview(&path, self.start, self.end, width, height)?;
        self.preview = Some(preview);
        self.playing = true;
        self.play_started = Some(Instant::now());
        self.playhead = self.start;
        Ok(())
    }

    /// Kills the decoder and forgets the frame pipe. The current frame is
    /// kept, so the box freezes on the last decoded image.
    pub fn stop_preview(&mut self) {
        self.playing = false;
        self.play_started = None;

        if let Some(preview) = self.preview.take() {
            if let Ok(mut slot) = preview.child.lock() {
                if let Some(mut child) = slot.take() {
                    let _ = child.kill();
                }
            }
            // Dropping the receiver makes the worker's next send fail, which
            // is its cue to shut the decoder down.
        }
    }

    /// The selected range, as consumed by the compression pass. `None` when
    /// the whole clip is selected, which lets compression keep its copy
    /// fast path.
    pub fn range(&self) -> Option<(f64, f64)> {
        if self.duration <= 0.0 || self.source.is_none() {
            return None;
        }

        let full = self.start <= 0.001 && self.end >= self.duration - 0.001;
        if full {
            None
        } else {
            Some((self.start, self.end))
        }
    }
}

// ---------------------------------------------------------------------------
// Duration probe
// ---------------------------------------------------------------------------

/// Reads the clip's duration and resolution from ffmpeg's own banner, so no
/// ffprobe binary has to ship next to ffmpeg.
pub fn probe_clip(path: &Path) -> Option<ProbeInfo> {
    let ffmpeg = crate::videos::ffmpeg_path()?;

    let mut command = std::process::Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(crate::videos::CREATE_NO_WINDOW);
    }

    // With no output argument ffmpeg prints the input's details and exits
    // with an error; the banner is all we need.
    let output = command.output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    let duration = parse_duration_line(&stderr)?;
    let (width, height) = parse_resolution(&stderr)?;

    Some(ProbeInfo {
        duration,
        width,
        height,
    })
}

/// `Duration: 00:00:07.03, start: ...` → 7.03
fn parse_duration_line(stderr: &str) -> Option<f64> {
    let line = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("Duration: "))?;
    let value = line.split("Duration: ").nth(1)?.split(',').next()?.trim();

    let mut parts = value.split(':');
    let hours: f64 = parts.next()?.trim().parse().ok()?;
    let minutes: f64 = parts.next()?.trim().parse().ok()?;
    let seconds: f64 = parts.next()?.trim().parse().ok()?;

    (hours >= 0.0 && minutes >= 0.0 && seconds >= 0.0)
        .then(|| hours * 3600.0 + minutes * 60.0 + seconds)
}

/// The first `WIDTHxHEIGHT` token on the video stream's banner line.
fn parse_resolution(stderr: &str) -> Option<(u32, u32)> {
    let line = stderr
        .lines()
        .find(|line| line.starts_with("  Stream #") && line.contains("Video:"))?;

    line.split_whitespace().find_map(|token| {
        let (width, height) = token.split_once('x')?;
        let width = width.parse::<u32>().ok()?;
        let height = height.parse::<u32>().ok()?;
        (width > 0 && height > 0).then_some((width, height))
    })
}

// ---------------------------------------------------------------------------
// Preview worker
// ---------------------------------------------------------------------------

/// Spawns the decoder worker and returns the handles for it.
fn spawn_preview(
    path: &Path,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
) -> Result<PreviewProcess, String> {
    let Some(ffmpeg) = crate::videos::ffmpeg_path() else {
        return Err(tr("error-ffmpeg-missing"));
    };

    let frame_bytes = (width as usize) * (height as usize) * 4;
    let (sender, receiver) = mpsc::sync_channel(4);
    let child = Arc::new(Mutex::new(None));
    let path = path.to_path_buf();

    let mut command = std::process::Command::new(ffmpeg);
    command
        .args(preview_args(&path, start, end, width, height))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(crate::videos::CREATE_NO_WINDOW);
    }

    let mut process = command
        .spawn()
        .map_err(|error| tr_arg("error-ffmpeg-start", "error", error.to_string()))?;

    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| tr_arg("error-ffmpeg-start", "error", tr("word-unknown-error")))?;

    if let Ok(mut slot) = child.lock() {
        *slot = Some(process);
    }

    let worker_child = child.clone();
    std::thread::spawn(move || {
        let mut frame = vec![0u8; frame_bytes];
        let mut reader = std::io::BufReader::new(stdout);

        loop {
            match reader.read_exact(&mut frame) {
                Ok(()) => {
                    let event = PreviewEvent::Frame(PreviewFrame {
                        width,
                        height,
                        pixels: frame.clone(),
                    });

                    // A full pipe or a dropped receiver means the UI is gone
                    // or stalled; kill the decoder rather than queueing.
                    if sender.send(event).is_err() {
                        kill_ffmpeg(&worker_child);
                        break;
                    }
                }
                Err(error) => {
                    let event = if error.kind() == std::io::ErrorKind::UnexpectedEof {
                        PreviewEvent::Ended
                    } else {
                        PreviewEvent::Failed(error.to_string())
                    };
                    let _ = sender.send(event);
                    break;
                }
            }
        }

        reap(&worker_child);
    });

    Ok(PreviewProcess {
        rx: Arc::new(Mutex::new(receiver)),
        child,
    })
}

/// Command line for the preview pass: raw RGBA frames scaled to fit the
/// preview box, read at native speed with `-re`.
fn preview_args(path: &Path, start: f64, end: f64, width: u32, height: u32) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-ss".to_string(),
        format!("{start:.3}"),
        "-re".to_string(),
        "-i".to_string(),
        path.to_string_lossy().into_owned(),
        "-t".to_string(),
        format!("{:.3}", end - start),
        "-an".to_string(),
        "-vf".to_string(),
        format!("scale={width}:{height}"),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        "-".to_string(),
    ]
}

/// Kills the live ffmpeg process, if it is still registered.
fn kill_ffmpeg(child: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Ok(mut slot) = child.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
        }
    }
}

/// Reaps the finished process so no zombie handles pile up while the panel
/// is open.
fn reap(child: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Ok(mut slot) = child.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duration_banner() {
        let banner = "  Duration: 00:00:30.03, start: 0.000000, bitrate: 3978 kb/s";
        assert_eq!(parse_duration_line(banner), Some(30.03));
    }

    #[test]
    fn parses_video_resolution() {
        let banner = "  Stream #0:0[0x1](und): Video: h264 (avc1 / 0x31637661), \
                      yuv420p(progressive), 1922x1112 [SAR 1:1 DAR 961:556], 3975 kb/s";
        assert_eq!(parse_resolution(banner), Some((1922, 1112)));
    }

    #[test]
    fn fits_wide_video_to_preview() {
        // 1922x1112 downscales to 415x240, keeping the aspect ratio.
        assert_eq!(fit(1922, 1112, 436, 240), (415, 240));
    }

    #[test]
    fn fits_small_video_with_upscale() {
        // 320x180 grows to 427x240; height wins because it is the tighter
        // dimension.
        assert_eq!(fit(320, 180, 436, 240), (427, 240));
    }
}
