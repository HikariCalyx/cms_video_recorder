// win32.rs – Windows-specific window tweaks
//
// Two effects are applied once the window exists:
//
//   1. Uniform translucency via WS_EX_LAYERED + LWA_ALPHA.
//   2. Rounded corners via SetWindowRgn with a round-rect region.
//
// Region clipping is used instead of LWA_COLORKEY because the colour key
// requires an *exact* framebuffer match, and colours written through the wgpu
// surface go through sRGB conversion — so the key was never matching and the
// corners stayed opaque. Clipping is pixel-exact and needs no colour matching.

use iced::window::raw_window_handle::{RawWindowHandle, WindowHandle};

use windows::Win32::{
    Foundation::{BOOL, COLORREF, HWND, RECT},
    Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn},
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            GetWindowLongPtrW, GetWindowRect, SetLayeredWindowAttributes, SetWindowLongPtrW,
            GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        },
    },
};

/// Overall window opacity (0 = invisible, 255 = fully opaque).
pub const WINDOW_ALPHA: u8 = 238;

/// Corner radius in logical pixels. Keep in sync with the pill's border radius
/// so the drawn border lines up with the clipped edge.
pub const CORNER_RADIUS: f32 = 12.0;

/// Make the window translucent and clip it to a rounded rectangle.
pub fn apply_window_effects(handle: WindowHandle<'_>) {
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };

    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);

    unsafe {
        // Uniform alpha for the whole window
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as isize);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), WINDOW_ALPHA, LWA_ALPHA);

        apply_rounded_region(hwnd);
    }
}

/// Clip the window to a round-rect region sized in physical pixels.
unsafe fn apply_rounded_region(hwnd: HWND) {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }

    // The region is in physical pixels, so scale the logical radius by DPI.
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let diameter = (CORNER_RADIUS * 2.0 * scale).round().max(1.0) as i32;

    // CreateRoundRectRgn's bottom/right bounds are exclusive.
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
    if region.is_invalid() {
        return;
    }

    // On success the window takes ownership of the region.
    let _ = SetWindowRgn(hwnd, region, BOOL(1));
}
