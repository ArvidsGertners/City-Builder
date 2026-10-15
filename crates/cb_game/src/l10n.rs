//! Bevy resource around `cb_l10n` and locale selection (ADR-0021).

use bevy::ecs::resource::Resource;
use cb_l10n::{L10n, Locale};

use crate::settings::Language;

/// The game's localization.
#[derive(Resource, Debug)]
pub struct Localization(pub L10n);

/// The OS locale from the usual environment variables (`LC_ALL` > `LC_MESSAGES` > `LANG`).
/// Windows and Finder-launched macOS apps usually have none; then English is used unless the
/// settings say otherwise (OS API lookup is a known gap until M15).
#[must_use]
pub fn system_locale(env: &dyn Fn(&str) -> Option<String>) -> Option<Locale> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(env)
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .and_then(|v| Locale::from_code(&v))
}

/// `--language` > settings > OS locale > English.
#[must_use]
pub fn choose_locale(cli: Option<&str>, setting: Language, system: Option<Locale>) -> Locale {
    if let Some(l) = cli.and_then(Locale::from_code) {
        return l;
    }
    match setting {
        Language::English => Locale::En,
        Language::German => Locale::De,
        Language::Auto => system.unwrap_or(Locale::En),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_precedence() {
        let env = |k: &str| (k == "LANG").then(|| "de_AT.UTF-8".to_owned());
        assert_eq!(system_locale(&env), Some(Locale::De));
        assert_eq!(system_locale(&|_| Some("C".to_owned())), None);
        assert_eq!(
            choose_locale(Some("en"), Language::German, Some(Locale::De)),
            Locale::En
        );
        assert_eq!(choose_locale(None, Language::German, None), Locale::De);
        assert_eq!(
            choose_locale(None, Language::Auto, Some(Locale::De)),
            Locale::De
        );
        assert_eq!(choose_locale(Some("xx"), Language::Auto, None), Locale::En);
    }
}
