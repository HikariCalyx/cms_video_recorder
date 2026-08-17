// settings.rs – state for the settings panel
//
// The panel edits text, so the fields it owns are strings rather than the typed
// values in `AppConfig`. They are parsed on commit, which keeps a half-typed
// number from being interpreted as a real setting.

use std::path::PathBuf;

use iced::keyboard::{key::Named, Key, Modifiers};

use crate::config::{AppConfig, Hotkey};

/// What a captured key press means for the hotkey field.
pub enum Capture {
    /// A usable combination.
    Bound(Hotkey),
    /// Clear the hotkey.
    Cleared,
    /// Leave the hotkey as it was.
    Cancelled,
    /// A modifier on its own, or a key that can't be bound – keep waiting.
    Ignored,
}

#[derive(Debug, Default)]
pub struct SettingsState {
    /// Whether the panel is expanded.
    pub is_open: bool,
    /// Output directory, as typed.
    pub output_dir: String,
    /// Duration cap in seconds, as typed. Empty counts as zero.
    pub max_duration: String,
    /// The shortcut being edited, `None` when disabled.
    pub hotkey: Option<Hotkey>,
    /// True while the next key press is being read as the new shortcut.
    pub capturing: bool,
    /// True while the folder dialog is open, so the button can't be pressed
    /// twice.
    pub browsing: bool,
}

impl SettingsState {
    /// Fills the edit fields from `config`.
    pub fn reset(&mut self, config: &AppConfig) {
        self.output_dir = config.output_dir.to_string_lossy().into_owned();
        self.max_duration = config.max_duration_secs.to_string();
        self.hotkey = config.hotkey;
        self.capturing = false;
    }

    /// Builds a config from the edit fields.
    ///
    /// A blank or unparseable duration becomes zero, i.e. no limit, and a blank
    /// path falls back to the default output directory.
    pub fn to_config(&self) -> AppConfig {
        let trimmed = self.output_dir.trim();

        AppConfig {
            output_dir: if trimmed.is_empty() {
                crate::recorder::default_output_dir()
            } else {
                PathBuf::from(trimmed)
            },
            max_duration_secs: self.max_duration.trim().parse().unwrap_or(0),
            hotkey: self.hotkey,
        }
    }

}

/// Turns a key press into a hotkey.
///
/// A bare letter would be swallowed system-wide, so a modifier is required for
/// anything that isn't a function key.
pub fn capture(key: &Key, modifiers: Modifiers) -> Capture {
    match key {
        // Escape and Backspace are the panel's controls, not bindable keys.
        Key::Named(Named::Escape) => return Capture::Cancelled,
        Key::Named(Named::Backspace | Named::Delete) if modifiers.is_empty() => {
            return Capture::Cleared
        }
        _ => {}
    }

    let Some(vk) = virtual_key(key) else {
        return Capture::Ignored;
    };

    let is_function_key = (0x70..=0x87).contains(&vk);
    let hotkey = Hotkey {
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
        win: modifiers.logo(),
        key: vk,
    };

    if hotkey.modifier_bits() == 0 && !is_function_key {
        return Capture::Ignored;
    }

    Capture::Bound(hotkey)
}

/// Win32 virtual-key code for a key iced reported, when there is one.
///
/// Deliberately narrow: only keys that make sense as a global shortcut, and
/// only ones whose code doesn't depend on the keyboard layout.
fn virtual_key(key: &Key) -> Option<u32> {
    match key {
        Key::Character(text) => {
            let c = text.chars().next()?.to_ascii_uppercase();
            // Letters and digits share their ASCII value with their VK code.
            (c.is_ascii_uppercase() || c.is_ascii_digit()).then_some(c as u32)
        }
        Key::Named(named) => named_virtual_key(*named),
        Key::Unidentified => None,
    }
}

fn named_virtual_key(named: Named) -> Option<u32> {
    let vk = match named {
        Named::F1 => 0x70,
        Named::F2 => 0x71,
        Named::F3 => 0x72,
        Named::F4 => 0x73,
        Named::F5 => 0x74,
        Named::F6 => 0x75,
        Named::F7 => 0x76,
        Named::F8 => 0x77,
        Named::F9 => 0x78,
        Named::F10 => 0x79,
        Named::F11 => 0x7A,
        Named::F12 => 0x7B,
        Named::Space => 0x20,
        Named::Enter => 0x0D,
        Named::Tab => 0x09,
        Named::Insert => 0x2D,
        Named::Delete => 0x2E,
        Named::Backspace => 0x08,
        Named::Home => 0x24,
        Named::End => 0x23,
        Named::PageUp => 0x21,
        Named::PageDown => 0x22,
        Named::ArrowLeft => 0x25,
        Named::ArrowUp => 0x26,
        Named::ArrowRight => 0x27,
        Named::ArrowDown => 0x28,
        _ => return None,
    };

    Some(vk)
}
