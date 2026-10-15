//! Localization (ADR-0021): Fluent bundles for English and German embedded at compile time,
//! compile-checked message keys (`keys::*`, generated from `locales/en/*.ftl`), English
//! fallback with a counter, and locale-aware number/money/date formatting ([`format`]).
//!
//! Numbers are pre-formatted in Rust and passed to Fluent as text, so grouping and decimal
//! separators never depend on Fluent's number formatting.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

pub mod format;

/// A message ID. Use the generated constants in [`keys`]; a typo is a compile error.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key(pub &'static str);

impl Key {
    /// The Fluent message ID.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.0
    }
}

/// Generated message keys (one constant per English message ID, plus `ALL`).
#[allow(missing_docs)]
pub mod keys {
    include!(concat!(env!("OUT_DIR"), "/keys.rs"));
}

include!(concat!(env!("OUT_DIR"), "/resources.rs"));

/// A supported UI language.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Locale {
    /// English (fallback).
    #[default]
    En,
    /// German.
    De,
}

impl Locale {
    /// All supported locales; English first.
    pub const ALL: [Locale; 2] = [Locale::En, Locale::De];

    /// Short code used for directories and settings (`"en"`, `"de"`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::De => "de",
        }
    }

    /// The language's own name for menus.
    #[must_use]
    pub const fn native_name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::De => "Deutsch",
        }
    }

    /// Locale for a code like `de`, `de-AT`, `de_DE.UTF-8` (case-insensitive); `None` if
    /// unsupported.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Locale> {
        let lang: String = code
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match lang.as_str() {
            "en" => Some(Locale::En),
            "de" => Some(Locale::De),
            _ => None,
        }
    }

    fn langid(self) -> LanguageIdentifier {
        match self {
            Locale::En => unic_langid::langid!("en-US"),
            Locale::De => unic_langid::langid!("de-DE"),
        }
    }

    const fn index(self) -> usize {
        match self {
            Locale::En => 0,
            Locale::De => 1,
        }
    }
}

/// A message argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg<'a> {
    /// Text (already formatted numbers go here).
    Text(Cow<'a, str>),
    /// An integer, usable for plural selection.
    Int(i64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Arg::Text(Cow::Borrowed(s))
    }
}

impl From<String> for Arg<'_> {
    fn from(s: String) -> Self {
        Arg::Text(Cow::Owned(s))
    }
}

impl From<i64> for Arg<'_> {
    fn from(v: i64) -> Self {
        Arg::Int(v)
    }
}

/// Localization errors (all are build defects in the embedded resources).
#[derive(Debug, thiserror::Error)]
pub enum L10nError {
    /// A `.ftl` file failed to parse.
    #[error("{locale}/{file}: Fluent parse error: {details}")]
    Parse {
        /// Locale code.
        locale: &'static str,
        /// File name.
        file: &'static str,
        /// Parser messages.
        details: String,
    },
    /// A resource could not be added (e.g. duplicate message ID).
    #[error("{locale}/{file}: {details}")]
    Resource {
        /// Locale code.
        locale: &'static str,
        /// File name.
        file: &'static str,
        /// Bundle messages.
        details: String,
    },
}

/// Loaded bundles for every locale plus the active locale.
pub struct L10n {
    bundles: Vec<FluentBundle<FluentResource>>,
    locale: Locale,
    fallbacks: AtomicU64,
    missing: AtomicU64,
}

impl std::fmt::Debug for L10n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("L10n")
            .field("locale", &self.locale)
            .field("fallbacks", &self.fallback_count())
            .field("missing", &self.missing_count())
            .finish_non_exhaustive()
    }
}

impl L10n {
    /// Loads all embedded resources and activates `locale`.
    pub fn new(locale: Locale) -> Result<Self, L10nError> {
        let mut bundles = Vec::with_capacity(Locale::ALL.len());
        for loc in Locale::ALL {
            let mut bundle = FluentBundle::new_concurrent(vec![loc.langid()]);
            bundle.set_use_isolating(false);
            for &(code, file, source) in RESOURCES.iter().filter(|r| r.0 == loc.code()) {
                let resource =
                    FluentResource::try_new(source.to_owned()).map_err(|(_, errs)| {
                        L10nError::Parse {
                            locale: code,
                            file,
                            details: format!("{errs:?}"),
                        }
                    })?;
                bundle
                    .add_resource(resource)
                    .map_err(|errs| L10nError::Resource {
                        locale: code,
                        file,
                        details: format!("{errs:?}"),
                    })?;
            }
            bundles.push(bundle);
        }
        Ok(Self {
            bundles,
            locale,
            fallbacks: AtomicU64::new(0),
            missing: AtomicU64::new(0),
        })
    }

    /// The active locale.
    #[must_use]
    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// Switches the active locale (takes effect for the next lookup).
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// How many lookups fell back to English.
    #[must_use]
    pub fn fallback_count(&self) -> u64 {
        self.fallbacks.load(Ordering::Relaxed)
    }

    /// How many lookups found no message at all (the ID was shown).
    #[must_use]
    pub fn missing_count(&self) -> u64 {
        self.missing.load(Ordering::Relaxed)
    }

    /// Whether `locale` itself defines `key` (no fallback).
    #[must_use]
    pub fn has(&self, locale: Locale, key: Key) -> bool {
        self.bundles[locale.index()].has_message(key.id())
    }

    /// The message for `key` without arguments.
    #[must_use]
    pub fn tr(&self, key: Key) -> String {
        self.tr_args(key, &[])
    }

    /// The message for `key` with named arguments.
    #[must_use]
    pub fn tr_args(&self, key: Key, args: &[(&str, Arg<'_>)]) -> String {
        let fluent_args = (!args.is_empty()).then(|| {
            let mut fa = FluentArgs::with_capacity(args.len());
            for (name, value) in args {
                let v = match value {
                    Arg::Text(s) => FluentValue::from(s.to_string()),
                    Arg::Int(i) => FluentValue::from(*i),
                };
                fa.set(name.to_string(), v);
            }
            fa
        });
        if let Some(s) = self.format_in(self.locale, key, fluent_args.as_ref()) {
            return s;
        }
        if self.locale != Locale::En
            && let Some(s) = self.format_in(Locale::En, key, fluent_args.as_ref())
        {
            self.fallbacks.fetch_add(1, Ordering::Relaxed);
            return s;
        }
        self.missing.fetch_add(1, Ordering::Relaxed);
        key.id().to_owned()
    }

    fn format_in(&self, locale: Locale, key: Key, args: Option<&FluentArgs<'_>>) -> Option<String> {
        let bundle = &self.bundles[locale.index()];
        let pattern = bundle.get_message(key.id())?.value()?;
        let mut errors = Vec::new();
        let out = bundle.format_pattern(pattern, args, &mut errors);
        // Formatting errors (e.g. a missing argument) still produce readable text; they are
        // caught statically by `l10n_placeholders_match`.
        Some(out.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    /// Message ID → set of `$variables` used anywhere in its value or attributes.
    fn placeholders(source: &str) -> BTreeMap<String, BTreeSet<String>> {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut current: Option<String> = None;
        for line in source.lines() {
            let starts_message = line.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
            if starts_message {
                let (id, _) = line.split_once('=').expect("message line has '='");
                current = Some(id.trim().to_owned());
            } else if !line.starts_with(' ') {
                current = None;
            }
            let Some(id) = &current else { continue };
            let vars = out.entry(id.clone()).or_default();
            let mut rest = line;
            while let Some(pos) = rest.find('$') {
                let name: String = rest[pos + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                vars.insert(name);
                rest = &rest[pos + 1..];
            }
        }
        out
    }

    fn merged(locale: Locale) -> BTreeMap<String, BTreeSet<String>> {
        let mut all = BTreeMap::new();
        for &(code, _, source) in RESOURCES {
            if code == locale.code() {
                all.extend(placeholders(source));
            }
        }
        all
    }

    #[test]
    fn l10n_resources_parse() {
        L10n::new(Locale::En).unwrap();
    }

    #[test]
    fn l10n_keys_complete_en_de() {
        let l = L10n::new(Locale::De).unwrap();
        assert!(!keys::ALL.is_empty());
        for loc in Locale::ALL {
            for &key in keys::ALL {
                assert!(l.has(loc, key), "{} missing in {}", key.id(), loc.code());
            }
        }
        let en: BTreeSet<_> = merged(Locale::En).into_keys().collect();
        let de: BTreeSet<_> = merged(Locale::De).into_keys().collect();
        assert_eq!(en, de, "message ID sets differ between en and de");
    }

    #[test]
    fn l10n_placeholders_match() {
        let en = merged(Locale::En);
        let de = merged(Locale::De);
        for (id, vars) in &en {
            assert_eq!(Some(vars), de.get(id), "placeholders differ for {id}");
        }
    }

    #[test]
    fn l10n_tr_with_args() {
        let l = L10n::new(Locale::De).unwrap();
        let s = l.tr_args(
            keys::CALENDAR_DATE,
            &[
                ("year", Arg::Int(3)),
                ("season", "Frühling".into()),
                ("day", Arg::Int(2)),
            ],
        );
        assert_eq!(s, "Jahr 3, Frühling, Tag 2");
        assert_eq!(l.tr(keys::COMMON_CANCEL), "Abbrechen");
        assert_eq!(l.fallback_count(), 0);
    }

    #[test]
    fn l10n_missing_key_shows_id_and_counts() {
        let l = L10n::new(Locale::De).unwrap();
        assert_eq!(l.tr(Key("does-not-exist")), "does-not-exist");
        assert_eq!(l.missing_count(), 1);
    }

    #[test]
    fn locale_from_code_variants() {
        assert_eq!(Locale::from_code("de_DE.UTF-8"), Some(Locale::De));
        assert_eq!(Locale::from_code("EN-gb"), Some(Locale::En));
        assert_eq!(Locale::from_code("fr"), None);
        assert_eq!(Locale::from_code(""), None);
    }
}
