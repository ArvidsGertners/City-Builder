//! Player settings (ARCHITECTURE §14): presentation options only; sim-affecting options live
//! in `cb_sim::SimConfig`. Persisted as versioned RON by [`persist`].
//!
//! Enums are stored as plain strings so a damaged or outdated value can be detected and
//! replaced per field (see [`persist::load_from_str`]).

pub mod persist;

use bevy::ecs::resource::Resource;
use serde::{Deserialize, Serialize};

/// Current settings format version.
pub const SETTINGS_VERSION: u32 = 1;

/// Declares a fieldless enum serialized as a lowercase string.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)? } default $default:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
        #[serde(into = "String", try_from = "String")]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// All values.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The stored text.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text,)+ }
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.as_str().to_owned()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(s: String) -> Result<Self, String> {
                match s.as_str() {
                    $($text => Ok($name::$variant),)+
                    other => Err(format!("unknown value {other:?}")),
                }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name::$default
            }
        }
    };
}

string_enum!(
    /// Window mode (ADR-0053).
    WindowMode {
        /// A normal window.
        Windowed = "windowed",
        /// Borderless fullscreen.
        Borderless = "borderless",
        /// Exclusive fullscreen (Windows/Linux).
        Fullscreen = "fullscreen",
    } default Windowed
);

string_enum!(
    /// Graphics quality preset (PERFORMANCE §4).
    GraphicsPreset {
        /// Lowest.
        Low = "low",
        /// Apple M-series default.
        Medium = "medium",
        /// Reference-PC default.
        High = "high",
        /// Highest.
        Ultra = "ultra",
    } default High
);

string_enum!(
    /// Render scale (ADR-0053 exact fractions).
    RenderScale {
        /// Pick by GPU class.
        Auto = "auto",
        /// 1.
        Full = "1",
        /// 5/6.
        FiveSixths = "5/6",
        /// 3/4.
        ThreeQuarters = "3/4",
        /// 2/3.
        TwoThirds = "2/3",
        /// 3/5.
        ThreeFifths = "3/5",
        /// 1/2.
        Half = "1/2",
    } default Auto
);

string_enum!(
    /// UI language.
    Language {
        /// From the OS locale, English fallback.
        Auto = "auto",
        /// English.
        English = "en",
        /// German.
        German = "de",
    } default Auto
);

string_enum!(
    /// Colour-vision palette for overlays.
    ColorVision {
        /// Default palette.
        Standard = "standard",
        /// Red-green safe.
        Deuteranopia = "deuteranopia",
        /// Red-green safe (protan).
        Protanopia = "protanopia",
        /// Blue-yellow safe.
        Tritanopia = "tritanopia",
    } default Standard
);

/// Window and display.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    /// Mode.
    pub window_mode: WindowMode,
    /// Window width in logical pixels.
    pub width: u32,
    /// Window height in logical pixels.
    pub height: u32,
    /// Vertical sync.
    pub vsync: bool,
    /// Frame cap in FPS: 30, 60, 120 or 0 (unlimited).
    pub frame_cap: u32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            window_mode: WindowMode::Windowed,
            width: 1600,
            height: 900,
            vsync: true,
            frame_cap: 60,
        }
    }
}

/// Rendering quality.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    /// Preset.
    pub preset: GraphicsPreset,
    /// Render scale.
    pub render_scale: RenderScale,
}

/// Volumes in 0..=1.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// Master.
    pub master: f32,
    /// Ambient beds.
    pub ambient: f32,
    /// Effects.
    pub effects: f32,
    /// UI sounds.
    pub ui: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 0.8,
            ambient: 0.7,
            effects: 0.8,
            ui: 0.6,
        }
    }
}

/// Pacing and convenience.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GameplaySettings {
    /// Real minutes per game day at 1× (16, 24, 32, 48; pacing only, ADR-0042).
    pub day_length_min: u32,
    /// Freeze the lighting's time of day (the clock keeps running).
    pub lock_time_of_day: bool,
    /// Autosave interval in real minutes (0 = off).
    pub autosave_min: u32,
    /// Pan when the cursor touches the window edge.
    pub edge_pan: bool,
}

impl Default for GameplaySettings {
    fn default() -> Self {
        Self {
            day_length_min: 24,
            lock_time_of_day: false,
            autosave_min: 10,
            edge_pan: false,
        }
    }
}

/// Accessibility.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessibilitySettings {
    /// UI scale factor (0.75..=2.0) on top of the OS scale factor.
    pub ui_scale: f32,
    /// Reduce camera and UI motion.
    pub reduce_motion: bool,
    /// Overlay palette.
    pub color_vision: ColorVision,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            reduce_motion: false,
            color_vision: ColorVision::Standard,
        }
    }
}

/// All persisted settings.
#[derive(Resource, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Format version.
    pub version: u32,
    /// UI language.
    pub language: Language,
    /// Log filter (empty = default).
    pub log_filter: String,
    /// Display.
    pub display: DisplaySettings,
    /// Graphics.
    pub graphics: GraphicsSettings,
    /// Audio.
    pub audio: AudioSettings,
    /// Gameplay pacing.
    pub gameplay: GameplaySettings,
    /// Accessibility.
    pub accessibility: AccessibilitySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            language: Language::Auto,
            log_filter: String::new(),
            display: DisplaySettings::default(),
            graphics: GraphicsSettings::default(),
            audio: AudioSettings::default(),
            gameplay: GameplaySettings::default(),
            accessibility: AccessibilitySettings::default(),
        }
    }
}

impl Settings {
    /// Clamps or resets out-of-range values; returns a warning per change.
    pub fn sanitize(&mut self) -> Vec<String> {
        let d = Settings::default();
        let mut w = Vec::new();
        let mut fix = |ok: bool, name: &str, reset: &mut dyn FnMut()| {
            if !ok {
                w.push(format!("settings: {name} out of range, using default"));
                reset();
            }
        };
        let disp = &mut self.display;
        fix(
            (320..=16_384).contains(&disp.width),
            "display.width",
            &mut || disp.width = d.display.width,
        );
        fix(
            (240..=16_384).contains(&disp.height),
            "display.height",
            &mut || disp.height = d.display.height,
        );
        fix(
            [0, 30, 60, 120].contains(&disp.frame_cap),
            "display.frame_cap",
            &mut || {
                disp.frame_cap = d.display.frame_cap;
            },
        );
        let g = &mut self.gameplay;
        fix(
            [16, 24, 32, 48].contains(&g.day_length_min),
            "gameplay.day_length_min",
            &mut || {
                g.day_length_min = d.gameplay.day_length_min;
            },
        );
        fix(g.autosave_min <= 120, "gameplay.autosave_min", &mut || {
            g.autosave_min = d.gameplay.autosave_min;
        });
        let a = &mut self.audio;
        for (name, v, def) in [
            ("audio.master", &mut a.master, d.audio.master),
            ("audio.ambient", &mut a.ambient, d.audio.ambient),
            ("audio.effects", &mut a.effects, d.audio.effects),
            ("audio.ui", &mut a.ui, d.audio.ui),
        ] {
            fix((0.0..=1.0).contains(&*v), name, &mut || *v = def);
        }
        let acc = &mut self.accessibility;
        fix(
            (0.75..=2.0).contains(&acc.ui_scale),
            "accessibility.ui_scale",
            &mut || {
                acc.ui_scale = d.accessibility.ui_scale;
            },
        );
        w
    }

    /// One-line summary for crash reports.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "lang={} mode={} {}x{} vsync={} cap={} preset={} scale={} day={}min lock_tod={}",
            self.language.as_str(),
            self.display.window_mode.as_str(),
            self.display.width,
            self.display.height,
            self.display.vsync,
            self.display.frame_cap,
            self.graphics.preset.as_str(),
            self.graphics.render_scale.as_str(),
            self.gameplay.day_length_min,
            self.gameplay.lock_time_of_day,
        )
    }
}
