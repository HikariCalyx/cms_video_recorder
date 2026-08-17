// icon.rs – extracts a window's icon as RGBA pixels via Win32/GDI
//
// The icon is taken from the window itself (WM_GETICON, falling back to the
// window class icon) rather than from the executable file. That avoids needing
// the Win32_UI_Shell feature and gives the icon the window actually displays.

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    },
    UI::WindowsAndMessaging::{
        GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, GCLP_HICONSM, HICON,
        ICONINFO, ICON_BIG, ICON_SMALL, SMTO_ABORTIFHUNG, WM_GETICON,
    },
};

/// Decoded icon bitmap, ready to hand to iced as an image.
pub struct IconRgba {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, top-down
    pub pixels: Vec<u8>,
}

/// Resolve and decode the icon for `hwnd`, if it has one.
pub fn icon_for_window(hwnd: isize) -> Option<IconRgba> {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        let hicon = query_window_icon(hwnd)?;
        // Note: icons obtained via WM_GETICON / GCLP_HICON are owned by the
        // window, so they must NOT be destroyed here.
        decode_icon(hicon)
    }
}

/// Ask the window for its icon, then fall back to its class icon.
unsafe fn query_window_icon(hwnd: HWND) -> Option<HICON> {
    // WM_GETICON with a timeout so a hung game can't block the UI thread.
    for kind in [ICON_BIG, ICON_SMALL] {
        let mut result = 0usize;
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(kind as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            120,
            Some(&mut result),
        );
        if ok.0 != 0 && result != 0 {
            return Some(HICON(result as *mut core::ffi::c_void));
        }
    }

    // Class icon as a fallback
    for index in [GCLP_HICON, GCLP_HICONSM] {
        let handle = GetClassLongPtrW(hwnd, index);
        if handle != 0 {
            return Some(HICON(handle as *mut core::ffi::c_void));
        }
    }

    None
}

/// Convert an HICON's colour bitmap into top-down RGBA bytes.
unsafe fn decode_icon(hicon: HICON) -> Option<IconRgba> {
    let mut info = ICONINFO::default();
    GetIconInfo(hicon, &mut info).ok()?;

    // Release the bitmaps GetIconInfo handed us on every exit path.
    let cleanup = |info: &ICONINFO| {
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask);
        }
    };

    if info.hbmColor.is_invalid() {
        // Monochrome icon – not worth rendering
        cleanup(&info);
        return None;
    }

    let mut bmp = BITMAP::default();
    let read = GetObjectW(
        info.hbmColor,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut BITMAP as *mut core::ffi::c_void),
    );
    if read == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
        cleanup(&info);
        return None;
    }

    let width = bmp.bmWidth as u32;
    let height = bmp.bmHeight as u32;

    let mut header = BITMAPINFO::default();
    header.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bmp.bmWidth,
        // Negative height requests a top-down DIB, matching iced's expectation
        biHeight: -bmp.bmHeight,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let mut buffer = vec![0u8; (width * height * 4) as usize];

    let hdc = GetDC(None);
    let scanlines = GetDIBits(
        hdc,
        info.hbmColor,
        0,
        height,
        Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
        &mut header,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, hdc);
    cleanup(&info);

    if scanlines == 0 {
        return None;
    }

    // GDI returns BGRA; iced wants RGBA.
    let mut opaque_pixels = 0usize;
    for px in buffer.chunks_exact_mut(4) {
        px.swap(0, 2);
        if px[3] != 0 {
            opaque_pixels += 1;
        }
    }

    // Some 32-bit icons carry an all-zero alpha channel, which would render
    // fully invisible. Treat those as opaque.
    if opaque_pixels == 0 {
        for px in buffer.chunks_exact_mut(4) {
            px[3] = 0xFF;
        }
    }

    Some(IconRgba {
        width,
        height,
        pixels: buffer,
    })
}
