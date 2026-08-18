// i18n.rs – user-visible strings
//
// The FTL resources in i18n/*.ftl are embedded at compile time and one
// FluentBundle is built per supported language on first `init`. `tr` reads
// whichever bundle the atomic index points at, so it can be called from any
// thread – including the capture, audio and compression workers, whose error
// strings are all routed through here too.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use fluent::{concurrent::FluentBundle, FluentArgs, FluentResource};
use serde::{Deserialize, Serialize};

const EN: &str = include_str!("../i18n/en.ftl");
const ZH_CN: &str = include_str!("../i18n/zh-CN.ftl");
const ZH_TW: &str = include_str!("../i18n/zh-TW.ftl");

/// Languages the UI can be displayed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
    #[serde(rename = "en")]
    English,
}

impl Language {
    /// Every supported language, in bundle order.
    pub const ALL: [Language; 3] = [
        Language::SimplifiedChinese,
        Language::TraditionalChinese,
        Language::English,
    ];

    fn index(self) -> usize {
        match self {
            Language::SimplifiedChinese => 0,
            Language::TraditionalChinese => 1,
            Language::English => 2,
        }
    }

    /// The BCP-47 code the config file stores.
    pub fn code(self) -> &'static str {
        match self {
            Language::SimplifiedChinese => "zh-CN",
            Language::TraditionalChinese => "zh-TW",
            Language::English => "en",
        }
    }

    /// The language's own name, for the settings picker.
    pub fn label(self) -> &'static str {
        match self {
            Language::SimplifiedChinese => "简体中文",
            Language::TraditionalChinese => "繁體中文",
            Language::English => "English",
        }
    }

    /// The best supported language for the user's OS locale, defaulting to
    /// Simplified Chinese for locales we don't ship.
    pub fn from_os() -> Language {
        os_locale()
            .as_deref()
            .and_then(Language::from_code)
            .unwrap_or(Language::SimplifiedChinese)
    }

    /// Maps a BCP-47-ish locale name to a supported language.
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.to_ascii_lowercase();

        if code.starts_with("en") {
            return Some(Language::English);
        }

        if code.starts_with("zh") {
            let traditional = ["tw", "hk", "mo", "hant"]
                .iter()
                .any(|tag| code.contains(tag));
            return Some(if traditional {
                Language::TraditionalChinese
            } else {
                Language::SimplifiedChinese
            });
        }

        None
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::from_os()
    }
}

/// The OS's UI locale name, e.g. "zh-CN" or "en-US".
#[cfg(windows)]
fn os_locale() -> Option<String> {
    // LOCALE_NAME_MAX_LENGTH is 85, including the NUL terminator.
    let mut buffer = [0u16; 85];
    let written = unsafe {
        windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buffer)
    };
    (written > 0).then(|| String::from_utf16_lossy(&buffer[..written as usize]))
}

#[cfg(not(windows))]
fn os_locale() -> Option<String> {
    std::env::var("LANG")
        .ok()
        .or_else(|| std::env::var("LC_ALL").ok())
}

// ---------------------------------------------------------------------------
// Bundles
// ---------------------------------------------------------------------------

type Bundle = FluentBundle<FluentResource>;

struct L10n {
    /// Index of the currently selected language, into `bundles`.
    current: AtomicUsize,
    bundles: [Bundle; 3],
}

static L10N: OnceLock<L10n> = OnceLock::new();

/// Builds the bundles and selects `language`. A cheap no-op on later calls.
pub fn init(language: Language) {
    L10N.get_or_init(|| L10n {
        current: AtomicUsize::new(Language::SimplifiedChinese.index()),
        bundles: Language::ALL.map(build_bundle),
    });
    set(language);
}

/// Switches the UI language without rebuilding the bundles.
pub fn set(language: Language) {
    if let Some(l10n) = L10N.get() {
        l10n.current.store(language.index(), Ordering::Release);
    }
}

/// The currently selected language.
pub fn current() -> Language {
    let index = L10N
        .get()
        .map(|l10n| l10n.current.load(Ordering::Acquire))
        .unwrap_or(0);
    Language::ALL[index]
}

fn build_bundle(language: Language) -> Bundle {
    let source = match language {
        Language::SimplifiedChinese => ZH_CN,
        Language::TraditionalChinese => ZH_TW,
        Language::English => EN,
    };

    let mut bundle = Bundle::new_concurrent(vec![
        language.code().parse().expect("valid language tag"),
    ]);

    // A parse failure here means a typo in a shipped file, which is worth
    // failing fast over: silently dropping every translation would be worse.
    let resource =
        FluentResource::try_new(source.to_string()).expect("failed to parse a bundled FTL file");
    bundle
        .add_resource(resource)
        .expect("failed to add FTL resource to a bundle");
    bundle
}

fn lookup(l10n: &L10n, language: Language, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    let bundle = &l10n.bundles[language.index()];
    let message = bundle.get_message(id)?;
    let pattern = message.value()?;

    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors).into_owned();
    (!text.is_empty()).then_some(text)
}

/// The message `id` in the current language, falling back to English and then
/// to the id itself so a missing key can never blank the UI.
pub fn tr(id: &str) -> String {
    let l10n = L10N.get().expect("i18n used before init");
    let language = current();

    lookup(l10n, language, id, None)
        .or_else(|| lookup(l10n, Language::English, id, None))
        .unwrap_or_else(|| id.to_string())
}

/// Like [`tr`], with one `{ $key }` argument filled in.
pub fn tr_arg(id: &str, key: &str, value: impl AsRef<str>) -> String {
    let l10n = L10N.get().expect("i18n used before init");
    let language = current();

    let mut args = FluentArgs::new();
    args.set(key, value.as_ref());

    lookup(l10n, language, id, Some(&args))
        .or_else(|| lookup(l10n, Language::English, id, Some(&args)))
        .unwrap_or_else(|| id.to_string())
}
