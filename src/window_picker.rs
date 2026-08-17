// window_picker.rs – enumerates selectable top-level windows via Win32

#[cfg(windows)]
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GA_ROOT, GWL_EXSTYLE,
        WS_EX_TOOLWINDOW,
    },
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A window that can be picked as a capture target
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Raw HWND value – used by the capture backend
    pub hwnd: isize,
    pub title: String,
    /// Client size in physical pixels, for display in the list
    pub width: i32,
    pub height: i32,
    /// Whether the window is currently minimised
    pub minimized: bool,
}

impl WindowInfo {
    /// Short "1920×1080" style description
    pub fn dimensions(&self) -> String {
        if self.minimized {
            "minimised".to_string()
        } else {
            format!("{}\u{00D7}{}", self.width, self.height)
        }
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

    /// (Re-)enumerate selectable windows
    pub fn refresh(&mut self) {
        self.windows = enumerate_windows();
    }

    /// Label shown on the toolbar button
    pub fn button_label(&self) -> &str {
        match &self.selected {
            Some(info) => info.title.as_str(),
            None => "Select Window",
        }
    }
}

// ---------------------------------------------------------------------------
// Win32 enumeration
// ---------------------------------------------------------------------------

/// Returns visible, titled, non-cloaked top-level windows belonging to other
/// processes, sorted by title.
#[cfg(windows)]
pub fn enumerate_windows() -> Vec<WindowInfo> {
    let mut result: Vec<WindowInfo> = Vec::new();
    let ptr = &mut result as *mut Vec<WindowInfo> as isize;

    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(ptr));
    }

    result.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    result
}

#[cfg(not(windows))]
pub fn enumerate_windows() -> Vec<WindowInfo> {
    Vec::new()
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    const CONTINUE: BOOL = BOOL(1);

    if !is_capturable(hwnd) {
        return CONTINUE;
    }

    let Some(title) = window_title(hwnd) else {
        return CONTINUE;
    };

    let mut rect = RECT::default();
    let (width, height) = if GetWindowRect(hwnd, &mut rect).is_ok() {
        (rect.right - rect.left, rect.bottom - rect.top)
    } else {
        (0, 0)
    };

    let list = &mut *(lparam.0 as *mut Vec<WindowInfo>);
    list.push(WindowInfo {
        hwnd: hwnd.0 as isize,
        title,
        width,
        height,
        minimized: IsIconic(hwnd).as_bool(),
    });

    CONTINUE
}

/// Filters out the noise that `EnumWindows` reports: invisible windows, tool
/// windows, DWM-cloaked UWP ghosts, child windows and our own toolbar.
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

    // Skip our own window so the toolbar can't record itself
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == GetCurrentProcessId() {
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
