// videos.rs – the recordings manager
//
// Lists the MP4 files in the configured output directory and offers per-clip
// actions: play in the default player, compress (placeholder), reveal in
// Explorer, and delete.

use std::path::{Path, PathBuf};
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
        Err(format!("ShellExecute 返回 {}", code.0 as isize))
    }
}

#[cfg(not(windows))]
pub fn play(_path: &Path) -> Result<(), String> {
    Err("当前平台不支持".to_string())
}

#[cfg(not(windows))]
pub fn browse(_path: &Path) -> Result<(), String> {
    Err("当前平台不支持".to_string())
}
