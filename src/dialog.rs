// dialog.rs – native folder picker for the output directory
//
// Uses the shell's IFileOpenDialog in folder-picking mode rather than the old
// SHBrowseForFolder, so the user gets the normal Explorer chrome: a path bar,
// the sidebar, and "new folder".
//
// The dialog is modal and pumps its own messages, so it runs on a worker thread
// with its own apartment. That keeps the toolbar drawing while it is open, and
// keeps COM initialisation away from the thread iced runs on.

use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows::core::{HSTRING, PCWSTR};
#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED,
    },
    UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    },
};

/// Asks the user for a folder, starting at `current`.
///
/// Returns `None` when the dialog is cancelled or the shell refuses to open it.
/// Must be called from a thread that has no apartment yet – i.e. a thread
/// spawned for this call.
#[cfg(windows)]
pub fn pick_folder(current: &Path) -> Option<PathBuf> {
    unsafe {
        // STA, because the dialog is a window and needs a message pump.
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return None;
        }

        let picked = show(current);

        CoUninitialize();
        picked
    }
}

/// No shell dialog off Windows, where recording isn't supported anyway.
#[cfg(not(windows))]
pub fn pick_folder(_current: &Path) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
unsafe fn show(current: &Path) -> Option<PathBuf> {
    let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;

    dialog
        .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
        .ok()?;
    let _ = dialog.SetTitle(&HSTRING::from("选择保存位置"));

    // Opening on the current directory only works once it exists; before the
    // first recording it usually doesn't, and the shell picks its own default.
    let start = HSTRING::from(current);
    if let Ok(folder) =
        SHCreateItemFromParsingName::<PCWSTR, _, IShellItem>(PCWSTR(start.as_ptr()), None)
    {
        let _ = dialog.SetFolder(&folder);
    }

    // Cancelling comes back as HRESULT_FROM_WIN32(ERROR_CANCELLED), so any
    // error here means "no folder chosen".
    dialog.Show(HWND::default()).ok()?;

    let item = dialog.GetResult().ok()?;
    let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;

    // The shell allocated the string; free it either way.
    let path = display.to_string().ok().map(PathBuf::from);
    CoTaskMemFree(Some(display.0 as *const core::ffi::c_void));

    path
}
