// videos.rs – the recordings manager
//
// Lists the MP4 files in the configured output directory and offers per-clip
// actions: play in the default player, compress with the bundled ffmpeg,
// reveal in Explorer, and delete.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One recorded clip, as shown in the manager.
#[derive(Debug, Clone)]
pub struct VideoEntry {
    /// Full path of the MP4 on disk.
    pub path: PathBuf,
    /// File name, e.g. `冒险岛正式服_20260817-203015.mp4`.
    pub name: String,
    /// Last-modified time, for sorting newest first.
    pub modified: Option<SystemTime>,
    /// File size in bytes.
    pub size: u64,
}

impl VideoEntry {
    /// File name without the `.mp4` extension, for display.
    pub fn display_name(&self) -> &str {
        display_name(&self.name)
    }
}

/// Strips the extension from a file name, for display.
///
/// Every entry is an MP4, so the extension only adds noise to the list.
/// The `Path` round-trip handles whatever casing Windows reported.
pub fn display_name(name: &str) -> &str {
    Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name)
}

/// State of the recordings panel.
#[derive(Debug, Default)]
pub struct VideosState {
    /// Whether the panel is expanded.
    pub is_open: bool,
    /// Clips currently in the output directory, newest first.
    pub videos: Vec<VideoEntry>,
    /// Clip waiting for its deletion to be confirmed.
    pub pending_delete: Option<PathBuf>,
    /// Clip being compressed on the worker thread.
    pub compressing: Option<PathBuf>,
    /// Cancellation handle for the running pass, if any.
    pub session: Option<CompressionSession>,
}

impl VideosState {
    /// Re-scan the output directory.
    pub fn refresh(&mut self, dir: &Path) {
        self.videos = enumerate(dir);
    }
}

/// MP4 files in `dir`, newest first.
///
/// A missing or unreadable directory yields an empty list, which the panel
/// renders as its "nothing here" state.
pub fn enumerate(dir: &Path) -> Vec<VideoEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut videos: Vec<VideoEntry> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let is_file = entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false);
            let is_mp4 = entry
                .path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("mp4"))
                .unwrap_or(false);

            if !is_file || !is_mp4 {
                return None;
            }

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = std::fs::metadata(&path).ok();
            let modified = metadata
                .as_ref()
                .and_then(|meta| meta.modified().ok());
            let size = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);

            Some(VideoEntry {
                path,
                name,
                modified,
                size,
            })
        })
        .collect();

    videos.sort_by(|a, b| b.modified.cmp(&a.modified));
    videos
}

/// Human-readable file size in binary units: `123 B`, `5.1 KiB`, `2.3 MiB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Permanently deletes a clip.
pub fn delete(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

/// Compressed size the pass walks down to, in bytes: 5000 KB.
const TARGET_BYTES: u64 = 5000 * 1024;
/// Starting video bitrate, in kbps: 5 Mbps.
const START_BITRATE: i64 = 5000;
/// How much each retry drops the bitrate.
const BITRATE_STEP: i64 = 200;
/// Floor below which the video isn't worth keeping.
const MIN_BITRATE: i64 = 200;

/// ffmpeg is a console app; without this a console window would flash for
/// every pass. Shared with the trim preview, which also spawns ffmpeg.
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a compression pass produced.
#[derive(Debug, Clone, Copy)]
pub struct CompressionOutcome {
    /// Size of the compressed file, in bytes.
    pub size: u64,
    /// True when the 5000 KB target was reached.
    pub reached_target: bool,
}

/// What the worker reports back once it is done, however it ended.
#[derive(Debug, Clone)]
pub enum CompressionReport {
    /// The destination file is ready.
    Done(CompressionOutcome),
    /// ffmpeg failed; the destination is untouched.
    Failed(String),
    /// The user pressed cancel; the partial file was removed.
    Cancelled,
}

/// Shared handle between the UI thread and the encode worker.
///
/// The UI keeps one to cancel; the worker registers the live ffmpeg process
/// in it so `cancel` can kill it mid-pass.
#[derive(Debug, Default, Clone)]
pub struct CompressionSession {
    child: Arc<Mutex<Option<std::process::Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl CompressionSession {
    /// Requests cancellation and kills the running ffmpeg process, if any.
    ///
    /// The worker notices the flag at its next poll, tidies the temp file and
    /// reports back [`CompressionReport::Cancelled`].
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);

        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                // Dropping the handle closes the process handle; `kill` has
                // already fired, so the UI thread never blocks on `wait`.
            }
        }
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Re-encodes `source` into `destination` with ffmpeg, walking the bitrate
/// down from 5 Mbps in 200 kbps steps until the file shrinks under 5000 KB.
///
/// `range` trims the pass to a segment, in seconds: `-ss` seeks the input
/// and `-t` bounds the output. The source is only ever read, never modified:
/// ffmpeg writes a temporary file next to the destination and it is renamed
/// over on success. Runs to completion before returning, so it belongs on a
/// worker thread. The shared `session` lets the UI cancel mid-pass.
pub fn compress_to(
    source: &Path,
    destination: &Path,
    session: &CompressionSession,
    range: Option<(f64, f64)>,
) -> CompressionReport {
    let Some(ffmpeg) = ffmpeg_path() else {
        return CompressionReport::Failed(crate::i18n::tr("error-ffmpeg-missing"));
    };

    // Nothing to gain from re-encoding an already-small clip; copy it
    // straight across. A requested trim still has to re-encode, since the
    // output is a different segment.
    let current = std::fs::metadata(source)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if range.is_none() && current < TARGET_BYTES {
        if source != destination {
            if let Err(error) = std::fs::copy(source, destination) {
                return CompressionReport::Failed(crate::i18n::tr_arg(
                    "error-copy-file",
                    "error",
                    error.to_string(),
                ));
            }
        }

        return CompressionReport::Done(CompressionOutcome {
            size: current,
            reached_target: true,
        });
    }

    let temp = temp_path(destination);
    let _ = std::fs::remove_file(&temp);

    let mut bitrate = START_BITRATE;

    loop {
        if session.is_cancelled() {
            let _ = std::fs::remove_file(&temp);
            return CompressionReport::Cancelled;
        }

        match run_ffmpeg(&ffmpeg, source, &temp, bitrate, range, session) {
            PassOutcome::Done => {}
            PassOutcome::Failed(error) => {
                let _ = std::fs::remove_file(&temp);
                return CompressionReport::Failed(error);
            }
            PassOutcome::Cancelled => {
                let _ = std::fs::remove_file(&temp);
                return CompressionReport::Cancelled;
            }
        }

        let size = match std::fs::metadata(&temp) {
            Ok(meta) => meta.len(),
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                return CompressionReport::Failed(crate::i18n::tr_arg(
                    "error-read-result",
                    "error",
                    error.to_string(),
                ));
            }
        };

        if size < TARGET_BYTES {
            return match finalize(destination, &temp, size, true) {
                Ok(outcome) => CompressionReport::Done(outcome),
                Err(error) => CompressionReport::Failed(error),
            };
        }

        bitrate -= BITRATE_STEP;
        if bitrate < MIN_BITRATE {
            // The floor was reached without meeting the target. Keep the
            // smallest result instead of throwing the work away.
            return match finalize(destination, &temp, size, false) {
                Ok(outcome) => CompressionReport::Done(outcome),
                Err(error) => CompressionReport::Failed(error),
            };
        }
    }
}

/// Replaces the original with the compressed file.
///
/// Both live in the same directory, and `fs::rename` maps to
/// `MoveFileEx(..., MOVEFILE_REPLACE_EXISTING)` on Windows, so the original
/// is swapped out in one step.
fn finalize(
    path: &Path,
    temp: &Path,
    size: u64,
    reached_target: bool,
) -> Result<CompressionOutcome, String> {
    std::fs::rename(temp, path).map_err(|error| {
        crate::i18n::tr_arg("error-replace-file", "error", error.to_string())
    })?;
    Ok(CompressionOutcome { size, reached_target })
}

/// The temporary file ffmpeg writes into: `clip.mp4.compress`, same folder.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip.mp4".to_string());

    path.with_file_name(format!("{name}.compress"))
}

#[cfg(windows)]
const FFMPEG_NAME: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_NAME: &str = "ffmpeg";

/// Locates the bundled ffmpeg: next to the executable, in an `ffmpeg`
/// subfolder next to it, or anywhere on `PATH` as a development fallback.
///
/// `None` means the file is missing – the caller shows the user where to put
/// it back.
pub fn ffmpeg_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf);

    if let Some(dir) = exe_dir {
        let direct = dir.join(FFMPEG_NAME);
        if direct.is_file() {
            return Some(direct);
        }

        let nested = dir.join("ffmpeg").join(FFMPEG_NAME);
        if nested.is_file() {
            return Some(nested);
        }
    }

    for entry in std::env::split_paths(&std::env::var_os("PATH")?) {
        let candidate = entry.join(FFMPEG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Outcome of one ffmpeg pass.
enum PassOutcome {
    Done,
    Failed(String),
    Cancelled,
}

/// One ffmpeg pass.
///
/// The process is registered in `session` so the UI thread can kill it, and
/// stderr is drained on a side thread so the pipe can never fill up and stall
/// ffmpeg.
fn run_ffmpeg(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    bitrate_kbps: i64,
    range: Option<(f64, f64)>,
    session: &CompressionSession,
) -> PassOutcome {
    use std::io::Read;

    let mut command = std::process::Command::new(ffmpeg);
    command
        .args(ffmpeg_args(input, output, bitrate_kbps, range))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return PassOutcome::Failed(crate::i18n::tr_arg(
                "error-ffmpeg-start",
                "error",
                error.to_string(),
            ));
        }
    };

    let stderr_reader = child.stderr.take().map(|pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let mut reader = std::io::BufReader::new(pipe);
            let _ = reader.read_to_end(&mut buffer);
            buffer
        })
    });

    // Publish the live handle so the UI thread can cancel.
    if let Ok(mut slot) = session.child.lock() {
        *slot = Some(child);
    } else {
        return PassOutcome::Failed(crate::i18n::tr("error-compress-state"));
    }

    // Poll for completion, letting go of the lock between polls so a cancel
    // can reach the child.
    let status = loop {
        if session.is_cancelled() {
            break None;
        }

        let mut slot = match session.child.lock() {
            Ok(slot) => slot,
            Err(_) => return PassOutcome::Failed(crate::i18n::tr("error-compress-state")),
        };

        match slot.as_mut() {
            // `cancel` already took the handle and killed the process.
            None => break None,
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(error) => {
                    return PassOutcome::Failed(crate::i18n::tr_arg(
                        "error-ffmpeg-wait",
                        "error",
                        error.to_string(),
                    ));
                }
            },
        }

        drop(slot);
        std::thread::sleep(std::time::Duration::from_millis(100));
    };

    // Reap the finished handle from the slot.
    if let Ok(mut slot) = session.child.lock() {
        slot.take();
    }

    let stderr = stderr_reader
        .map(|handle| handle.join().ok())
        .flatten()
        .unwrap_or_default();

    if session.is_cancelled() {
        return PassOutcome::Cancelled;
    }

    match status {
        Some(status) if status.success() => PassOutcome::Done,
        _ => {
            let error = ffmpeg_error(&stderr);

            // The toolbar status can only hold a one-line summary, so debug
            // builds also dump the full transcript to the console.
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "--- {} ---",
                    crate::i18n::tr_arg("error-ffmpeg-encode", "error", &error)
                );
                eprintln!("{}", String::from_utf8_lossy(&stderr));
            }

            PassOutcome::Failed(crate::i18n::tr_arg("error-ffmpeg-encode", "error", error))
        }
    }
}

/// Command line for one pass. Video is re-encoded to the requested bitrate;
/// audio is copied through untouched.
///
/// Input seeking (`-ss` before `-i`) is fast and keyframe-accurate for the
/// re-encoded video; `-t` after the input bounds the output to the selected
/// segment.
///
/// `-f mp4` matters: the temporary file ends in `.compress`, not `.mp4`, so
/// without an explicit format ffmpeg has no extension to guess a muxer from.
fn ffmpeg_args(
    input: &Path,
    output: &Path,
    bitrate_kbps: i64,
    range: Option<(f64, f64)>,
) -> Vec<String> {
    let mut args = vec!["-y".to_string()];

    if let Some((start, _)) = range {
        args.push("-ss".to_string());
        args.push(format!("{start:.3}"));
    }

    args.push("-i".to_string());
    args.push(input.to_string_lossy().into_owned());

    if let Some((start, end)) = range {
        args.push("-t".to_string());
        args.push(format!("{:.3}", end - start));
    }

    args.extend([
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "fast".to_string(),
        "-b:v".to_string(),
        format!("{bitrate_kbps}k"),
        "-c:a".to_string(),
        "copy".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-f".to_string(),
        "mp4".to_string(),
        output.to_string_lossy().into_owned(),
    ]);

    args
}

/// Last non-empty line of ffmpeg's stderr, clipped to a readable length.
fn ffmpeg_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let fallback = crate::i18n::tr("word-unknown-error");
    let tail = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(&fallback);

    tail.chars().take(120).collect()
}

/// Opens the clip with the shell's default handler for `.mp4`.
#[cfg(windows)]
pub fn play(path: &Path) -> Result<(), String> {
    shell_open(path, None)
}

/// Opens an Explorer window with the clip selected.
#[cfg(windows)]
pub fn browse(path: &Path) -> Result<(), String> {
    // `/select` needs the full path, quoted so spaces survive Explorer's
    // argument parsing. `"` can't appear in a Windows file name, so the
    // quotes are always safe.
    let parameters = format!("/select,\"{}\"", path.display());
    shell_open(Path::new("explorer.exe"), Some(&parameters))
}

/// Runs a shell verb against a file, without a console window.
///
/// `ShellExecute` is used rather than a spawned `cmd`, which would flash a
/// console whenever the default player needs one.
#[cfg(windows)]
fn shell_open(path: &Path, parameters: Option<&str>) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::{
        Foundation::HWND,
        UI::Shell::ShellExecuteW,
        UI::WindowsAndMessaging::SW_SHOWNORMAL,
    };

    let verb = HSTRING::from("open");
    let file = HSTRING::from(path);
    let parameters = parameters.map(HSTRING::from);

    let code = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            parameters
                .as_ref()
                .map(|params| PCWSTR(params.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // The shell reports failure as a small integer (<= 32), not an HRESULT.
    if code.0 as isize > 32 {
        Ok(())
    } else {
        Err(crate::i18n::tr_arg(
            "error-shellexecute",
            "code",
            (code.0 as isize).to_string(),
        ))
    }
}

#[cfg(not(windows))]
pub fn play(_path: &Path) -> Result<(), String> {
    Err(crate::i18n::tr("error-unsupported-platform"))
}

#[cfg(not(windows))]
pub fn browse(_path: &Path) -> Result<(), String> {
    Err(crate::i18n::tr("error-unsupported-platform"))
}
