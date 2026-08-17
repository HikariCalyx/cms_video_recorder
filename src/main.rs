// CMS Video Recorder – floating toolbar
// Windows-only desktop application built with iced 0.13

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod icon;
mod recorder;
mod toolbar;
#[cfg(windows)]
mod win32;
mod window_picker;

use iced::{application::Appearance, window, Color, Theme};
use toolbar::{Toolbar, UI_FONT};


fn main() -> iced::Result {
    let window_settings = window::Settings {
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
        .run()
}
