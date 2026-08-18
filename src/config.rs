// config.rs – user settings, persisted as JSON
//
// Lives at %APPDATA%\cms_video_recorder\config.json. The file is written
// whenever a setting is committed and read once at startup; anything missing
// or malformed falls back to the default, so a hand-edited file can never stop
// the app from launching.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::recorder::{self, RecorderConfig};

/// Folder created under `%APPDATA%` for the config file.
const CONFIG_DIR: &str = "cms_video_recorder";
const CONFIG_FILE: &str = "config.json";

// ---------------------------------------------------------------------------
// Hotkey
// ---------------------------------------------------------------------------

/// A system-wide shortcut, stored as Win32 modifier flags plus a virtual-key
/// code so it round-trips through `RegisterHotKey` without a lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    /// The Windows key. Rarely a good idea – the shell claims most Win
    /// combinations – but there is no reason to forbid it.
    #[serde(default)]
    pub win: bool,
    /// Win32 virtual-key code, e.g. `0x52` for R.
    pub key: u32,
}

// Win32 modifier flags, as `RegisterHotKey` wants them.
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

impl Hotkey {
    /// Ctrl + Shift + R, the shipped default.
    pub const DEFAULT: Self = Self {
        ctrl: true,
        alt: false,
        shift: true,
        win: false,
        key: 0x52,
    };

    /// Encoding of "no hotkey", for the atomic the listener thread reads.
    pub const NONE_BITS: u64 = 0;

    /// Modifier flags for `RegisterHotKey`.
    pub fn modifier_bits(&self) -> u32 {
        let mut bits = 0;
        if self.ctrl {
            bits |= MOD_CONTROL;
        }
        if self.alt {
            bits |= MOD_ALT;
        }
        if self.shift {
            bits |= MOD_SHIFT;
        }
        if self.win {
            bits |= MOD_WIN;
        }
        bits
    }

    /// Packs an optional binding into one `u64` so it can live in an atomic
    /// and be handed to the listener thread without a lock.
    pub fn to_bits(hotkey: Option<Self>) -> u64 {
        match hotkey {
            Some(hotkey) if hotkey.key != 0 => {
                (hotkey.modifier_bits() as u64) << 32 | hotkey.key as u64
            }
            _ => Self::NONE_BITS,
        }
    }

    /// Inverse of [`Hotkey::to_bits`].
    pub fn from_bits(bits: u64) -> Option<Self> {
        let key = bits as u32;
        if key == 0 {
            return None;
        }

        let modifiers = (bits >> 32) as u32;
        Some(Self {
            ctrl: modifiers & MOD_CONTROL != 0,
            alt: modifiers & MOD_ALT != 0,
            shift: modifiers & MOD_SHIFT != 0,
            win: modifiers & MOD_WIN != 0,
            key,
        })
    }

    /// The shortcut split into the names of the keys it uses, modifiers first,
    /// in the order Windows writes them.
    ///
    /// A key code with no name – which shouldn't happen, since the picker only
    /// accepts codes it can name – falls back to its hex value rather than
    /// disappearing from the display.
    pub fn parts(&self) -> Vec<String> {
        let mut parts: Vec<String> = Vec::with_capacity(5);

        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.win {
            parts.push("Win".to_string());
        }
        parts.push(key_label(self.key).unwrap_or_else(|| format!("0x{:02X}", self.key)));

        parts
    }

    /// Human-readable form, e.g. `Ctrl + Shift + R`.
    pub fn label(&self) -> String {
        self.parts().join(" + ")
    }
}

/// Name of a virtual-key code, for the keys the picker accepts.
pub fn key_label(vk: u32) -> Option<String> {
    let named = match vk {
        // Digits and letters share their ASCII value with their VK code.
        0x30..=0x39 | 0x41..=0x5A => return Some((vk as u8 as char).to_string()),
        0x70..=0x87 => return Some(format!("F{}", vk - 0x6F)),
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2D => "Insert",
        0x2E => "Delete",
        _ => return None,
    };

    Some(named.to_string())
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Everything the settings panel can change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Directory the MP4 files are written to.
    pub output_dir: PathBuf,
    /// Automatic stop, in seconds. Zero means no limit.
    pub max_duration_secs: u64,
    /// System-wide record/stop shortcut, or `None` to disable it.
    pub hotkey: Option<Hotkey>,
    /// Display language.
    pub language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_dir: recorder::default_output_dir(),
            max_duration_secs: recorder::MAX_DURATION.as_secs(),
            hotkey: Some(Hotkey::DEFAULT),
            language: Language::default(),
        }
    }
}

impl AppConfig {
    /// Reads the config file, falling back to defaults for anything the file
    /// doesn't provide.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };

        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };

        let mut config: Self = serde_json::from_str(&text).unwrap_or_default();

        // An empty path would send recordings to the working directory, which
        // for a shortcut launch is somewhere the user never looks.
        if config.output_dir.as_os_str().is_empty() {
            config.output_dir = recorder::default_output_dir();
        }

        config
    }

    /// Writes the config file, creating `%APPDATA%\cms_video_recorder` if it
    /// isn't there yet.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or_else(|| crate::i18n::tr("error-no-appdata"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(path, text).map_err(|error| error.to_string())
    }

    /// The duration cap as the recorder wants it: `None` for unlimited.
    pub fn max_duration(&self) -> Option<Duration> {
        (self.max_duration_secs > 0).then(|| Duration::from_secs(self.max_duration_secs))
    }

    /// Encoder settings for a recording started with these preferences.
    ///
    /// Everything the settings panel doesn't expose (bitrate, frame rate,
    /// warm-up) keeps its default.
    pub fn recorder_config(&self) -> RecorderConfig {
        RecorderConfig {
            output_dir: self.output_dir.clone(),
            max_duration: self.max_duration(),
            ..RecorderConfig::default()
        }
    }
}

/// `%APPDATA%\cms_video_recorder\config.json`.
///
/// `config_dir` resolves to the roaming AppData folder on Windows, which is
/// what `%APPDATA%` expands to.
pub fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(CONFIG_DIR).join(CONFIG_FILE))
}

/// Parent directory of the config file, for display.
pub fn config_dir_display() -> String {
    config_path()
        .as_deref()
        .and_then(Path::parent)
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default()
}
