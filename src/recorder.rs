// recorder.rs – window capture to MP4
//
// Capture goes through Windows.Graphics.Capture (the same API the Xbox Game
// Bar uses), and encoding through the Media Foundation transcoder that
// `windows-capture` wraps. That keeps the whole pipeline on the GPU: frames
// never round-trip to system memory, so recording a 1080p game window costs
// very little.
//
// The capture runs on its own thread. The UI holds a `Recording` handle and
// only touches it to read the elapsed time and to stop.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default ceiling on a single recording, until the settings panel says
/// otherwise.
///
/// Also drives the countdown on the Stop button, so the number the user sees
/// and the number the encoder enforces can't drift apart.
pub const MAX_DURATION: Duration = Duration::from_secs(30);

/// Sub-directory created inside the user's Videos folder.
const OUTPUT_SUBDIR: &str = "cms_video_recorder";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Recording settings.
///
/// The output directory and duration cap come from the settings panel by way of
/// `AppConfig`; everything else is still taken from `Default`.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Directory the MP4 files are written to. Created on demand.
    pub output_dir: PathBuf,
    /// Recording stops automatically after this long. `None` runs until the
    /// user stops it.
    pub max_duration: Option<Duration>,
    /// Encoder frame rate hint, in frames per second.
    pub frame_rate: u32,
    /// Target video bitrate, in bits per second.
    pub bitrate: u32,
    /// Record only the window's client area, dropping the title bar and
    /// borders.
    ///
    /// Costs a GPU-to-system-memory copy per frame, because the encoder only
    /// takes a GPU surface for the frame exactly as captured. Windows with no
    /// chrome to strip stay on the zero-copy path regardless of this setting.
    pub exclude_window_border: bool,
    /// Largest capture that is recorded at native resolution, as
    /// `(width, height)`.
    ///
    /// Anything wider or taller is halved, so a 2560x1440 window records at
    /// 1280x720 and 3840x2160 at 1920x1080. `None` always records native.
    pub max_native_size: Option<(u32, u32)>,
    /// How long the capture pipeline is left to settle before frames are kept.
    ///
    /// Media Foundation and the hardware encoder take a moment to reach a
    /// steady state, and frames captured during that window land in the file as
    /// a frozen run at the head. Discarding them trades a little delay on the
    /// Record button for a clean opening.
    pub warmup: Duration,
    /// Record the audio coming out of the default playback device.
    ///
    /// This is the whole system mix, so anything else making noise lands in the
    /// clip too. If the device can't be opened the recording carries on without
    /// sound rather than failing.
    pub capture_audio: bool,
}

/// Frames captured within this long of the capture session starting are
/// discarded, so the recording opens on a settled pipeline.
pub const WARMUP: Duration = Duration::from_millis(600);

/// Captures wider or taller than this are halved.
///
/// Chosen so the game's 1920x1200 mode still records natively while the 1440p
/// and 4K modes come down to something a fixed bitrate can do justice to.
pub const MAX_NATIVE_SIZE: (u32, u32) = (1920, 1200);

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            max_duration: Some(MAX_DURATION),
            frame_rate: 60,
            // ~12 Mbps is plenty for a game window at 1080p60 and keeps a
            // 30 second clip under about 45 MB.
            bitrate: 12_000_000,
            exclude_window_border: true,
            max_native_size: Some(MAX_NATIVE_SIZE),
            warmup: WARMUP,
            capture_audio: true,
        }
    }
}

/// `%USERPROFILE%\Videos\cms_video_recorder`, falling back to the home
/// directory and finally the working directory if the shell folder can't be
/// resolved.
pub fn default_output_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OUTPUT_SUBDIR)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Anything that can go wrong while recording.
///
/// Backend errors are flattened to strings: they have to cross the capture
/// thread boundary, and the UI only ever shows the message.
#[derive(Debug, Clone)]
pub enum Error {
    /// Creating the output directory or file failed.
    Io(String),
    /// The encoder rejected a frame or failed to finalise the file.
    Encoder(String),
    /// Starting or stopping the capture session failed.
    Capture(String),
    /// Loopback audio capture failed. Never fatal – the recording falls back
    /// to video only.
    Audio(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "文件写入失败: {msg}"),
            Self::Encoder(msg) => write!(f, "编码失败: {msg}"),
            Self::Capture(msg) => write!(f, "录制失败: {msg}"),
            Self::Audio(msg) => write!(f, "音频录制失败: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

// ---------------------------------------------------------------------------
// Output naming
// ---------------------------------------------------------------------------

/// Builds a file name like `冒险岛正式服_20260817-203015.mp4`.
///
/// The alias is sanitised because it ends up in a path, and a local timestamp
/// down to the second is enough to keep successive recordings distinct given
/// the 30 second minimum spacing between them.
pub fn output_path(dir: &Path, alias: &str) -> PathBuf {
    let (y, mo, d, h, mi, s) = local_time();
    let stem = sanitize(alias);

    dir.join(format!("{stem}_{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.mp4"))
}

/// Strips characters Windows won't accept in a file name.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim_matches([' ', '.']).to_string();
    if trimmed.is_empty() {
        "recording".to_string()
    } else {
        trimmed
    }
}

/// Local wall-clock time as `(year, month, day, hour, minute, second)`.
#[cfg(windows)]
fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (
        now.wYear,
        now.wMonth,
        now.wDay,
        now.wHour,
        now.wMinute,
        now.wSecond,
    )
}

#[cfg(not(windows))]
fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    // Elapsed seconds since the epoch, good enough to keep names unique on
    // the non-Windows build, which can't record anyway.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (0, 0, 0, 0, 0, (secs % 60) as u16)
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod backend;

#[cfg(windows)]
pub use backend::Recording;

// --- Non-Windows stub ------------------------------------------------------

/// Placeholder so the rest of the app compiles off Windows.
#[cfg(not(windows))]
pub struct Recording;

#[cfg(not(windows))]
impl Recording {
    pub fn start(_hwnd: isize, _alias: &str, _config: &RecorderConfig) -> Result<Self, Error> {
        Err(Error::Capture("当前平台不支持录制".to_string()))
    }

    pub fn elapsed(&self) -> Duration {
        Duration::ZERO
    }

    pub fn target_hwnd(&self) -> isize {
        0
    }

    pub fn target_is_alive(&self) -> bool {
        false
    }

    pub fn finish(self) -> Result<PathBuf, Error> {
        Err(Error::Capture("当前平台不支持录制".to_string()))
    }
}
