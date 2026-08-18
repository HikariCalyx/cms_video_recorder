// CMS Video Recorder – floating toolbar
// Windows-only desktop application built with iced 0.13

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cog;
mod config;
mod dialog;
mod film;
mod hotkey;
#[cfg(windows)]
mod icon;
mod i18n;
mod recorder;
mod settings;
mod toolbar;
mod trim;
mod videos;
#[cfg(windows)]
mod win32;
mod window_picker;

use iced::{application::Appearance, window, Color, Theme};
use toolbar::{Toolbar, UI_FONT};

/// The same .ico that build.rs links in as a Win32 resource. The resource
/// covers Explorer / the taskbar's pinned entry; this copy is what we hand to
/// the windowing layer so the live window and Alt-Tab match.
const APP_ICON: &[u8] = include_bytes!("icon.ico");

fn main() -> iced::Result {
    // `None` lets the decoder sniff the format from the file header. A failure
    // here only costs us the icon, so it is not worth aborting startup over.
    let icon = window::icon::from_file_data(APP_ICON, None).ok();

    let window_settings = window::Settings {
        icon,
        size: iced::Size::new(toolbar::WINDOW_WIDTH, toolbar::BAR_HEIGHT),
        resizable: false,
        decorations: false,
        // We do NOT rely on wgpu per-pixel alpha; translucency is applied by
        // Win32 layered-window attributes instead (see win32.rs).
        transparent: false,
        level: window::Level::AlwaysOnTop,
        position: window::Position::Centered,
        min_size: None,
        max_size: None,
        ..Default::default()
    };

    iced::application("CMS Video Recorder", Toolbar::update, Toolbar::view)
        .window(window_settings)
        .theme(Toolbar::theme)
        .subscription(Toolbar::subscription)
        // Microsoft YaHei UI – a Windows system font with full CJK coverage,
        // resolved by name through the system font source.
        .default_font(UI_FONT)
        // Clear to the pill colour so no stray pixels peek out along the edge
        // that the Win32 region clips.
        .style(|_state: &Toolbar, _theme: &Theme| Appearance {
            background_color: toolbar::pill_bg(),
            text_color: Color::WHITE,
        })
        // Not `run()`: the initial state comes from the config file, so it
        // can't be built by `Default`.
        .run_with(Toolbar::new)
}
