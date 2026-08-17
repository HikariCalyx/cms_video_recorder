// window_picker.rs – enumerates selectable top-level windows via Win32

use iced::widget::image;

#[cfg(windows)]
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT},
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetAncestor, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GA_ROOT,
            GWL_EXSTYLE, WS_EX_TOOLWINDOW,
        },
    },
};

/// Executables that may be captured, each mapped to a display alias.
///
/// This table doubles as the allow-list: a window whose owning process is not
/// listed here is never offered as a capture target. Keys are compared
/// case-insensitively against the process image file name.
pub const PROCESS_ALIASES: &[(&str, &str)] = &[
    ("maplestory.exe", "冒险岛正式服"),
    ("maplestoryt.exe", "冒险岛测试服"),
    ("maplestoryta.exe", "冒险岛测试服"),
    ("maplestorym.exe", "冒险岛M"),
    ("maplestoryn.exe", "冒险岛N"),
    ("maplestory_classic.exe", "冒险岛怀旧服"),
    ("msw.exe", "冒险岛世界"),
];

/// Look up the display alias for an executable name.
pub fn alias_for(exe_name: &str) -> Option<&'static str> {
    PROCESS_ALIASES
        .iter()
        .find(|(exe, _)| exe_name.eq_ignore_ascii_case(exe))
        .map(|(_, alias)| *alias)
}

/// Executable name without the ".exe" suffix, e.g. "MapleStory".
pub fn strip_exe(name: &str) -> &str {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

/// Windows narrower than this are skipped – they're launchers, login prompts
/// and patcher dialogs rather than the game viewport.
///
/// Note this also excludes minimised windows, whose `GetWindowRect` reports a
/// small off-screen placeholder rather than the restored size.
pub const MIN_WINDOW_WIDTH: i32 = 800;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A window that can be picked as a capture target
///
/// `PartialEq` is deliberately not derived – identity is the `hwnd` alone, and
/// the icon handle should never participate in comparisons.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Raw HWND value – used by the capture backend
    pub hwnd: isize,
    pub title: String,
    /// Client size in physical pixels, for display in the list
    pub width: i32,
    pub height: i32,
    /// Whether the window is currently minimised
    pub minimized: bool,
    /// Owning executable name, e.g. "MapleStory.exe"
    pub process: String,
    /// Display label: the `PROCESS_ALIASES` server name when known, otherwise
    /// the process name without its extension.
    pub alias: String,
    /// The window's icon, decoded once per refresh
    pub icon: Option<image::Handle>,
}

impl WindowInfo {
    /// Short "1920×1080" style description
    pub fn dimensions(&self) -> String {
        if self.minimized {
            "已最小化".to_string()
        } else {
            format!("{}\u{00D7}{}", self.width, self.height)
        }
    }

    /// Executable name without the ".exe" suffix
    pub fn process_label(&self) -> &str {
        strip_exe(&self.process)
    }

    /// Secondary line: "MapleStoryTA · 1920×1080"
    ///
    /// The executable is included because MapleStoryT and MapleStoryTA share
    /// the same alias, so the alias alone can't distinguish them.
    pub fn detail(&self) -> String {
        format!("{} \u{00B7} {}", self.process_label(), self.dimensions())
    }
}

impl std::fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title)
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct WindowPickerState {
    /// Whether the picker panel is currently expanded
    pub is_open: bool,
    /// Currently selected capture target
    pub selected: Option<WindowInfo>,
    /// Cached list of enumerated windows
    pub windows: Vec<WindowInfo>,
    /// Whether windows from processes outside `PROCESS_ALIASES` are listed too
    pub include_others: bool,
}

impl WindowPickerState {
    /// Expand or collapse the panel; refresh the list when expanding
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if self.is_open {
            self.refresh();
        }
    }

    /// Confirm a selection and collapse the panel
    pub fn select(&mut self, info: WindowInfo) {
        self.selected = Some(info);
        self.is_open = false;
    }

    /// (Re-)enumerate selectable windows and decode their icons
    pub fn refresh(&mut self) {
        self.windows = enumerate_windows(self.include_others);
        self.attach_icons();

        // Drop a stale selection whose window has gone away, but keep the
        // freshly decoded icon/size for one that is still present.
        if let Some(selected) = &self.selected {
            match self.windows.iter().find(|w| w.hwnd == selected.hwnd) {
                Some(current) => self.selected = Some(current.clone()),
                None => self.selected = None,
            }
        }
    }

    /// Decode one icon per distinct executable and share it across windows.
    fn attach_icons(&mut self) {
        let mut cache: std::collections::HashMap<String, Option<image::Handle>> =
            std::collections::HashMap::new();

        for info in &mut self.windows {
            let key = info.process.to_lowercase();

            let handle = match cache.get(&key) {
                Some(cached) => cached.clone(),
                None => {
                    let decoded = decode_window_icon(info.hwnd);
                    cache.insert(key, decoded.clone());
                    decoded
                }
            };

            info.icon = handle;
        }
    }

    /// Label shown on the toolbar button – the alias once a window is picked
    pub fn button_label(&self) -> &str {
        match &self.selected {
            Some(info) => info.alias.as_str(),
            None => "选择窗口",
        }
    }

    /// Icon of the current selection, for the toolbar button
    pub fn selected_icon(&self) -> Option<&image::Handle> {
        self.selected.as_ref().and_then(|s| s.icon.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Win32 enumeration
// ---------------------------------------------------------------------------

/// Returns visible, titled, non-cloaked top-level windows, sorted by title.
///
/// With `include_others` off, only windows owned by a `PROCESS_ALIASES`
/// process are kept – that's the MapleStory family. With it on, every other
/// capturable window joins the list as well.
#[cfg(windows)]
pub fn enumerate_windows(include_others: bool) -> Vec<WindowInfo> {
    let mut context = EnumContext {
        windows: Vec::new(),
        include_others,
    };
    let ptr = &mut context as *mut EnumContext as isize;

    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(ptr));
    }

    context
        .windows
        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    context.windows
}

#[cfg(not(windows))]
pub fn enumerate_windows(_include_others: bool) -> Vec<WindowInfo> {
    Vec::new()
}

/// State threaded through the `EnumWindows` callback.
#[cfg(windows)]
struct EnumContext {
    windows: Vec<WindowInfo>,
    include_others: bool,
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    const CONTINUE: BOOL = BOOL(1);

    if !is_capturable(hwnd) {
        return CONTINUE;
    }

    // Allow-listed processes get their server alias. Any other process only
    // joins the list when the "list other windows" option is on, using its
    // bare executable name as the label.
    let Some(process) = owning_process_name(hwnd) else {
        return CONTINUE;
    };
    let context = &mut *(lparam.0 as *mut EnumContext);
    let alias = match alias_for(&process) {
        Some(alias) => alias.to_string(),
        None => {
            if !context.include_others {
                return CONTINUE;
            }
            strip_exe(&process).to_string()
        }
    };

    let Some(title) = window_title(hwnd) else {
        return CONTINUE;
    };

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return CONTINUE;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    if width < MIN_WINDOW_WIDTH {
        return CONTINUE;
    }

    let context = &mut *(lparam.0 as *mut EnumContext);
    context.windows.push(WindowInfo {
        hwnd: hwnd.0 as isize,
        title,
        width,
        height,
        minimized: IsIconic(hwnd).as_bool(),
        process,
        alias,
        // Filled in afterwards by `attach_icons`; decoding here would run
        // inside the enumeration callback for every candidate window.
        icon: None,
    });

    CONTINUE
}

/// Decode a window's icon into an iced image handle.
#[cfg(windows)]
fn decode_window_icon(hwnd: isize) -> Option<image::Handle> {
    let icon = crate::icon::icon_for_window(hwnd)?;
    Some(image::Handle::from_rgba(
        icon.width,
        icon.height,
        icon.pixels,
    ))
}

#[cfg(not(windows))]
fn decode_window_icon(_hwnd: isize) -> Option<image::Handle> {
    None
}

/// Resolves the executable file name that owns `hwnd`, e.g. "MapleStory.exe".
#[cfg(windows)]
unsafe fn owning_process_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }

    // LIMITED_INFORMATION is enough for the image name and, unlike full query
    // access, succeeds without elevation for most processes.
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

    let mut buf = [0u16; 260]; // MAX_PATH
    let mut len = buf.len() as u32;
    let query = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buf.as_mut_ptr()),
        &mut len,
    );
    let _ = CloseHandle(handle);
    query.ok()?;

    let full_path = String::from_utf16_lossy(&buf[..len as usize]);

    // Keep only the file name component
    full_path
        .rsplit(['\\', '/'])
        .next()
        .map(|name| name.to_string())
        .filter(|name| !name.is_empty())
}

/// Filters out the noise that `EnumWindows` reports: invisible windows, tool
/// windows, DWM-cloaked UWP ghosts and child windows. Process filtering is
/// handled separately by the allow-list.
#[cfg(windows)]
unsafe fn is_capturable(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }

    // Only true top-level windows
    if GetAncestor(hwnd, GA_ROOT) != hwnd {
        return false;
    }

    // Skip tool windows (palettes, tray helpers, ...)
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }

    // Skip DWM-cloaked windows – these are the invisible UWP/"ApplicationFrame"
    // shells that would otherwise clutter the list.
    let mut cloaked = 0u32;
    let ok = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
    if ok.is_ok() && cloaked != 0 {
        return false;
    }

    true
}

/// Reads a window's title, returning `None` when it is empty
#[cfg(windows)]
unsafe fn window_title(hwnd: HWND) -> Option<String> {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return None;
    }

    let mut buf = vec![0u16; (len + 1) as usize];
    let written = GetWindowTextW(hwnd, &mut buf);
    if written <= 0 {
        return None;
    }

    let title = String::from_utf16_lossy(&buf[..written as usize]);
    let trimmed = title.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
