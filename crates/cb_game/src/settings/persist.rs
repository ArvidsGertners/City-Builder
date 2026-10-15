//! Versioned RON persistence for [`Settings`] (ADR-0040 location, ARCHITECTURE §14).
//!
//! Loading never fails: an unreadable file gives defaults, and every field that is missing,
//! mistyped or out of range falls back to its default with a warning while the valid fields
//! are kept. A file from a newer version is backed up and replaced by defaults.

use std::path::Path;

use ron::Value;
use ron::value::Map;

use super::{SETTINGS_VERSION, Settings};

/// Where the loaded settings came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LoadSource {
    /// No file: defaults (first start).
    Defaults,
    /// Read from the file (possibly with per-field fallbacks).
    File,
    /// Migrated from an older version.
    Migrated {
        /// Version found in the file.
        from: u32,
    },
    /// The file is from a newer (unknown) version or unparseable: defaults.
    Reset {
        /// Version found in the file, if any.
        found: Option<u32>,
    },
}

/// Result of loading.
#[derive(Clone, Debug)]
pub struct LoadOutcome {
    /// Usable settings.
    pub settings: Settings,
    /// One message per fallback or problem (logged as warnings).
    pub warnings: Vec<String>,
    /// Origin.
    pub source: LoadSource,
}

const HEADER: &str = "// City Builder settings. Invalid values fall back to defaults on load.\n";

/// Serializes settings as pretty RON with a header comment.
#[must_use]
pub fn to_ron(settings: &Settings) -> String {
    let pretty = ron::ser::PrettyConfig::new()
        .struct_names(false)
        .indentor("    ".to_owned());
    match ron::ser::to_string_pretty(settings, pretty) {
        Ok(body) => format!("{HEADER}{body}\n"),
        // Settings contain only plain data; serialization cannot fail in practice.
        Err(e) => format!("{HEADER}// serialization failed: {e}\n"),
    }
}

fn default_value() -> Value {
    ron::to_string(&Settings::default())
        .ok()
        .and_then(|s| ron::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Map(Map::new()))
}

fn key(k: &str) -> Value {
    Value::String(k.to_owned())
}

fn key_name(k: &Value) -> String {
    match k {
        Value::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// Returns the value at `path` inside nested maps.
fn at_mut<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut cur = root;
    for p in path {
        match cur {
            Value::Map(m) => cur = m.get_mut(&key(p))?,
            _ => return None,
        }
    }
    Some(cur)
}

/// Overlays `file` onto `root` (a valid settings document) one leaf at a time, keeping a leaf
/// only if the whole document still deserializes.
fn overlay(root: &mut Value, path: &mut Vec<String>, file: &Map, warnings: &mut Vec<String>) {
    for (k, v) in file.iter() {
        let name = key_name(k);
        path.push(name);
        let dotted = path.join(".");
        let existing = at_mut(root, path).cloned();
        match (existing, v) {
            (None, _) => warnings.push(format!("settings: unknown field `{dotted}` ignored")),
            (Some(Value::Map(_)), Value::Map(sub)) => overlay(root, path, sub, warnings),
            (Some(old), new) => {
                if let Some(slot) = at_mut(root, path) {
                    *slot = new.clone();
                }
                if root.clone().into_rust::<Settings>().is_err() {
                    warnings.push(format!(
                        "settings: invalid value for `{dotted}`, using default"
                    ));
                    if let Some(slot) = at_mut(root, path) {
                        *slot = old;
                    }
                }
            }
        }
        path.pop();
    }
}

fn file_version(map: &Map) -> Option<u32> {
    match map.get(&key("version"))? {
        Value::Number(n) => u32::try_from(n.into_f64() as i64).ok(),
        _ => None,
    }
}

/// Parses settings text (see module docs for the fallback rules).
#[must_use]
pub fn load_from_str(text: &str) -> LoadOutcome {
    let mut warnings = Vec::new();
    let Ok(Value::Map(parsed)) = ron::from_str::<Value>(text) else {
        warnings.push("settings: file is not valid RON settings; using defaults".to_owned());
        return LoadOutcome {
            settings: Settings::default(),
            warnings,
            source: LoadSource::Reset { found: None },
        };
    };
    let version = file_version(&parsed);
    let source = match version {
        Some(v) if v > SETTINGS_VERSION => {
            warnings.push(format!(
                "settings: version {v} is newer than supported {SETTINGS_VERSION}; using defaults"
            ));
            return LoadOutcome {
                settings: Settings::default(),
                warnings,
                source: LoadSource::Reset { found: Some(v) },
            };
        }
        Some(v) if v == SETTINGS_VERSION => LoadSource::File,
        // No older format exists yet; future migrations go here (from → from + 1 steps).
        Some(v) => LoadSource::Migrated { from: v },
        None => {
            warnings.push("settings: missing version, assuming current".to_owned());
            LoadSource::File
        }
    };
    let mut root = default_value();
    overlay(&mut root, &mut Vec::new(), &parsed, &mut warnings);
    let mut settings = root.into_rust::<Settings>().unwrap_or_default();
    settings.version = SETTINGS_VERSION;
    warnings.extend(settings.sanitize());
    LoadOutcome {
        settings,
        warnings,
        source,
    }
}

/// Loads from `path`. A newer-version file is copied to `settings.ron.v<N>.bak` first.
#[must_use]
pub fn load(path: &Path) -> LoadOutcome {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoadOutcome {
                settings: Settings::default(),
                warnings: Vec::new(),
                source: LoadSource::Defaults,
            };
        }
        Err(e) => {
            return LoadOutcome {
                settings: Settings::default(),
                warnings: vec![format!(
                    "settings: cannot read {}: {e}; using defaults",
                    path.display()
                )],
                source: LoadSource::Reset { found: None },
            };
        }
    };
    let outcome = load_from_str(&text);
    if let LoadSource::Reset { found } = outcome.source {
        let suffix = found.map_or_else(|| "invalid".to_owned(), |v| format!("v{v}"));
        let _ = std::fs::write(path.with_extension(format!("ron.{suffix}.bak")), &text);
    }
    outcome
}

/// Writes atomically (temp file + rename).
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, to_ron(settings))?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    fn non_default() -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            language: Language::German,
            log_filter: "debug,wgpu=warn".into(),
            display: DisplaySettings {
                window_mode: WindowMode::Borderless,
                width: 2560,
                height: 1440,
                vsync: false,
                frame_cap: 120,
            },
            graphics: GraphicsSettings {
                preset: GraphicsPreset::Medium,
                render_scale: RenderScale::TwoThirds,
            },
            audio: AudioSettings {
                master: 0.5,
                ambient: 0.25,
                effects: 1.0,
                ui: 0.0,
            },
            gameplay: GameplaySettings {
                day_length_min: 48,
                lock_time_of_day: true,
                autosave_min: 0,
                edge_pan: true,
            },
            accessibility: AccessibilitySettings {
                ui_scale: 1.5,
                reduce_motion: true,
                color_vision: ColorVision::Tritanopia,
            },
        }
    }

    #[test]
    fn settings_roundtrip_all_fields() {
        let s = non_default();
        assert_ne!(s, Settings::default());
        let out = load_from_str(&to_ron(&s));
        assert_eq!(out.warnings, Vec::<String>::new());
        assert_eq!(out.source, LoadSource::File);
        assert_eq!(out.settings, s);
        // Defaults roundtrip too.
        assert_eq!(
            load_from_str(&to_ron(&Settings::default())).settings,
            Settings::default()
        );
    }

    #[test]
    fn settings_invalid_field_falls_back() {
        let text = to_ron(&non_default())
            .replace("window_mode: \"borderless\"", "window_mode: \"sideways\"")
            .replace("frame_cap: 120", "frame_cap: 77")
            .replace("vsync: false", "vsync: \"maybe\"")
            .replace("ui_scale: 1.5", "ui_scale: 9.0");
        let out = load_from_str(&format!("{text}\n"));
        let want = non_default();
        assert_eq!(out.settings.display.window_mode, WindowMode::Windowed);
        assert_eq!(out.settings.display.frame_cap, 60);
        assert!(out.settings.display.vsync);
        assert!((out.settings.accessibility.ui_scale - 1.0).abs() < f32::EPSILON);
        // Valid neighbours survive.
        assert_eq!(out.settings.display.width, want.display.width);
        assert_eq!(out.settings.graphics, want.graphics);
        assert_eq!(out.settings.gameplay, want.gameplay);
        assert_eq!(out.warnings.len(), 4, "{:?}", out.warnings);

        let partial = load_from_str("(version: 1, language: \"de\", extra: 5)");
        assert_eq!(partial.settings.language, Language::German);
        assert_eq!(partial.settings.display, DisplaySettings::default());
        assert_eq!(partial.warnings.len(), 1);

        let garbage = load_from_str("not ron at all {{{");
        assert_eq!(garbage.settings, Settings::default());
        assert_eq!(garbage.source, LoadSource::Reset { found: None });
    }

    #[test]
    fn settings_unknown_version_migrates_or_defaults() {
        let newer = load_from_str("(version: 99, language: \"de\")");
        assert_eq!(newer.source, LoadSource::Reset { found: Some(99) });
        assert_eq!(newer.settings, Settings::default());

        let older = load_from_str("(version: 0, language: \"de\")");
        assert_eq!(older.source, LoadSource::Migrated { from: 0 });
        assert_eq!(older.settings.language, Language::German);
        assert_eq!(older.settings.version, SETTINGS_VERSION);

        let dir = std::env::temp_dir().join(format!("cb-settings-{}", std::process::id()));
        let path = dir.join("settings.ron");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "(version: 99)").unwrap();
        let out = load(&path);
        assert_eq!(out.source, LoadSource::Reset { found: Some(99) });
        assert!(dir.join("settings.ron.v99.bak").is_file());
        save(&path, &non_default()).unwrap();
        assert_eq!(load(&path).settings, non_default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
