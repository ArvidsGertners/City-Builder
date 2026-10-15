//! Virtual `PRIMARY` modifier (Cmd on macOS, Ctrl elsewhere) and platform shortcut labels
//! (ADR-0052). Shortcuts are bound against [`Mods`]; physical modifier state is mapped once.

use super::Os;

/// A set of logical modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods(u8);

impl Mods {
    /// No modifiers.
    pub const NONE: Mods = Mods(0);
    /// Shift.
    pub const SHIFT: Mods = Mods(1);
    /// Alt / Option.
    pub const ALT: Mods = Mods(2);
    /// The platform's primary shortcut modifier: Cmd on macOS, Ctrl elsewhere.
    pub const PRIMARY: Mods = Mods(4);
    /// The other one of Ctrl/Cmd (Ctrl on macOS, Super/Win elsewhere); rarely bound.
    pub const SECONDARY: Mods = Mods(8);

    /// Union.
    #[must_use]
    pub const fn with(self, other: Mods) -> Mods {
        Mods(self.0 | other.0)
    }

    /// Whether all of `other` are set.
    #[must_use]
    pub const fn contains(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Physical modifier keys held.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PhysicalMods {
    /// Either Shift key.
    pub shift: bool,
    /// Either Ctrl key.
    pub ctrl: bool,
    /// Either Alt/Option key.
    pub alt: bool,
    /// Either Super/Cmd/Win key.
    pub super_key: bool,
}

/// Maps physical keys to logical modifiers for `os`.
#[must_use]
pub fn logical_mods(os: Os, p: PhysicalMods) -> Mods {
    let (primary, secondary) = match os {
        Os::MacOs => (p.super_key, p.ctrl),
        Os::Windows | Os::Linux => (p.ctrl, p.super_key),
    };
    let mut m = Mods::NONE;
    if p.shift {
        m = m.with(Mods::SHIFT);
    }
    if p.alt {
        m = m.with(Mods::ALT);
    }
    if primary {
        m = m.with(Mods::PRIMARY);
    }
    if secondary {
        m = m.with(Mods::SECONDARY);
    }
    m
}

/// A shortcut label: `⌘⇧Z` on macOS, `Ctrl+Shift+Z` elsewhere. `key` is already localized.
#[must_use]
pub fn shortcut_label(os: Os, mods: Mods, key: &str) -> String {
    let mut out = String::new();
    match os {
        Os::MacOs => {
            // Apple's canonical order: Control, Option, Shift, Command.
            for (m, glyph) in [
                (Mods::SECONDARY, "⌃"),
                (Mods::ALT, "⌥"),
                (Mods::SHIFT, "⇧"),
                (Mods::PRIMARY, "⌘"),
            ] {
                if mods.contains(m) {
                    out.push_str(glyph);
                }
            }
            out.push_str(key);
        }
        Os::Windows | Os::Linux => {
            let secondary = if os == Os::Windows { "Win" } else { "Super" };
            for (m, name) in [
                (Mods::PRIMARY, "Ctrl"),
                (Mods::SECONDARY, secondary),
                (Mods::ALT, "Alt"),
                (Mods::SHIFT, "Shift"),
            ] {
                if mods.contains(m) {
                    out.push_str(name);
                    out.push('+');
                }
            }
            out.push_str(key);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_is_cmd_on_macos_ctrl_elsewhere() {
        let cmd = PhysicalMods {
            super_key: true,
            ..PhysicalMods::default()
        };
        let ctrl = PhysicalMods {
            ctrl: true,
            ..PhysicalMods::default()
        };
        assert!(logical_mods(Os::MacOs, cmd).contains(Mods::PRIMARY));
        assert!(!logical_mods(Os::MacOs, ctrl).contains(Mods::PRIMARY));
        assert!(logical_mods(Os::Windows, ctrl).contains(Mods::PRIMARY));
        assert!(logical_mods(Os::Linux, ctrl).contains(Mods::PRIMARY));
        assert!(!logical_mods(Os::Linux, cmd).contains(Mods::PRIMARY));
    }

    #[test]
    fn shortcut_labels_per_platform() {
        let redo = Mods::PRIMARY.with(Mods::SHIFT);
        assert_eq!(shortcut_label(Os::MacOs, redo, "Z"), "⇧⌘Z");
        assert_eq!(shortcut_label(Os::Windows, redo, "Z"), "Ctrl+Shift+Z");
        assert_eq!(shortcut_label(Os::Linux, Mods::PRIMARY, "Z"), "Ctrl+Z");
        assert_eq!(shortcut_label(Os::MacOs, Mods::NONE, "B"), "B");
    }
}
