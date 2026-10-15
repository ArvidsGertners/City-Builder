//! Per-OS file locations (ADR-0040). [`resolve`] is pure: the environment, executable
//! directory and temp directory are injected, so all three layouts are tested everywhere.

use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;

use super::Os;

/// Every directory and file the game writes. Directories are created lazily on first write.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct AppPaths {
    /// `settings.ron`.
    pub settings_file: PathBuf,
    /// Saves and autosaves.
    pub saves_dir: PathBuf,
    /// Rolling log files.
    pub logs_dir: PathBuf,
    /// Crash reports and the `.last_crash` marker.
    pub crashes_dir: PathBuf,
    /// Player screenshots.
    pub screenshots_dir: PathBuf,
    /// Cache (reserved).
    pub cache_dir: PathBuf,
    /// How the layout was chosen.
    pub mode: PathMode,
}

/// How [`AppPaths`] were chosen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PathMode {
    /// The OS conventions.
    Standard,
    /// `portable.txt` next to the executable (or `.app`).
    Portable,
    /// `CB_USERDATA` override.
    Override,
    /// No home directory: everything under the temp dir (warn the player).
    TempFallback,
}

/// Inputs for [`resolve`].
pub struct PathInputs<'a> {
    /// Target OS.
    pub os: Os,
    /// Environment lookup.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Directory of the running executable, if known.
    pub exe_dir: Option<&'a Path>,
    /// Whether a file exists (for `portable.txt`).
    pub file_exists: &'a dyn Fn(&Path) -> bool,
    /// System temp directory.
    pub temp_dir: &'a Path,
}

const WIN_DIR: &str = "CityBuilder";
const XDG_DIR: &str = "city-builder";
const MAC_DIR: &str = "CityBuilder";

/// Resolves the layout: `CB_USERDATA` > portable mode > OS conventions > temp fallback.
#[must_use]
pub fn resolve(input: &PathInputs<'_>) -> AppPaths {
    let env = |k: &str| (input.env)(k).filter(|v| !v.is_empty());

    if let Some(dir) = env("CB_USERDATA") {
        return flat(Path::new(&dir), PathMode::Override);
    }
    if let Some(dir) = portable_root(input) {
        return flat(&dir.join("userdata"), PathMode::Portable);
    }

    match input.os {
        Os::Windows => {
            let profile = env("USERPROFILE").map(PathBuf::from);
            let roaming = env("APPDATA")
                .map(PathBuf::from)
                .or_else(|| profile.as_ref().map(|p| p.join("AppData").join("Roaming")));
            let local = env("LOCALAPPDATA")
                .map(PathBuf::from)
                .or_else(|| profile.as_ref().map(|p| p.join("AppData").join("Local")));
            match (roaming, local) {
                (Some(roaming), Some(local)) => {
                    let r = roaming.join(WIN_DIR);
                    let l = local.join(WIN_DIR);
                    AppPaths {
                        settings_file: r.join("settings.ron"),
                        saves_dir: r.join("saves"),
                        logs_dir: l.join("logs"),
                        crashes_dir: l.join("crashes"),
                        screenshots_dir: l.join("screenshots"),
                        cache_dir: l.join("cache"),
                        mode: PathMode::Standard,
                    }
                }
                _ => temp_fallback(input.temp_dir),
            }
        }
        Os::Linux => {
            let Some(home) = env("HOME").map(PathBuf::from) else {
                return temp_fallback(input.temp_dir);
            };
            // XDG: relative paths are invalid and ignored. (String check, so the Linux
            // layout is testable on Windows too.)
            let xdg = |var: &str, default: &[&str]| {
                env(var)
                    .filter(|v| v.starts_with('/'))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default.iter().fold(home.clone(), |p, part| p.join(part)))
            };
            let config = xdg("XDG_CONFIG_HOME", &[".config"]).join(XDG_DIR);
            let data = xdg("XDG_DATA_HOME", &[".local", "share"]).join(XDG_DIR);
            let state = xdg("XDG_STATE_HOME", &[".local", "state"]).join(XDG_DIR);
            let cache = xdg("XDG_CACHE_HOME", &[".cache"]).join(XDG_DIR);
            AppPaths {
                settings_file: config.join("settings.ron"),
                saves_dir: data.join("saves"),
                logs_dir: state.join("logs"),
                crashes_dir: state.join("crashes"),
                screenshots_dir: data.join("screenshots"),
                cache_dir: cache,
                mode: PathMode::Standard,
            }
        }
        Os::MacOs => {
            let Some(home) = env("HOME").map(PathBuf::from) else {
                return temp_fallback(input.temp_dir);
            };
            let support = home
                .join("Library")
                .join("Application Support")
                .join(MAC_DIR);
            let logs = home.join("Library").join("Logs").join(MAC_DIR);
            AppPaths {
                settings_file: support.join("settings.ron"),
                saves_dir: support.join("saves"),
                logs_dir: logs.clone(),
                crashes_dir: logs.join("crashes"),
                screenshots_dir: support.join("screenshots"),
                cache_dir: home.join("Library").join("Caches").join(MAC_DIR),
                mode: PathMode::Standard,
            }
        }
    }
}

/// The directory that holds `portable.txt`, if portable mode applies.
fn portable_root(input: &PathInputs<'_>) -> Option<PathBuf> {
    let exe_dir = input.exe_dir?;
    let root = match input.os {
        Os::MacOs => {
            // A quarantined app runs from a random read-only translocation mount: never
            // portable there (the user's portable.txt is not next to it).
            if exe_dir
                .components()
                .any(|c| c.as_os_str() == "AppTranslocation")
            {
                return None;
            }
            // `<dir>/CityBuilder.app/Contents/MacOS/<exe>` → `<dir>`.
            let is_bundle = exe_dir.ends_with("Contents/MacOS");
            match exe_dir.ancestors().nth(3) {
                Some(dir) if is_bundle => dir.to_path_buf(),
                _ => exe_dir.to_path_buf(),
            }
        }
        Os::Windows | Os::Linux => exe_dir.to_path_buf(),
    };
    (input.file_exists)(&root.join("portable.txt")).then_some(root)
}

fn flat(root: &Path, mode: PathMode) -> AppPaths {
    AppPaths {
        settings_file: root.join("settings.ron"),
        saves_dir: root.join("saves"),
        logs_dir: root.join("logs"),
        crashes_dir: root.join("crashes"),
        screenshots_dir: root.join("screenshots"),
        cache_dir: root.join("cache"),
        mode,
    }
}

fn temp_fallback(temp: &Path) -> AppPaths {
    flat(&temp.join(XDG_DIR), PathMode::TempFallback)
}

/// Resolves paths for the running process.
#[must_use]
pub fn resolve_current() -> AppPaths {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let env = |k: &str| std::env::var(k).ok();
    let exists = |p: &Path| p.is_file();
    let temp = std::env::temp_dir();
    resolve(&PathInputs {
        os: Os::current(),
        env: &env,
        exe_dir: exe_dir.as_deref(),
        file_exists: &exists,
        temp_dir: &temp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(os: Os, vars: &[(&str, &str)], exe: Option<&str>, portable: Option<&str>) -> AppPaths {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let env = move |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let portable = portable.map(PathBuf::from);
        let exists = move |p: &Path| portable.as_deref() == Some(p);
        let exe = exe.map(PathBuf::from);
        resolve(&PathInputs {
            os,
            env: &env,
            exe_dir: exe.as_deref(),
            file_exists: &exists,
            temp_dir: Path::new("/tmp"),
        })
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn paths_resolve_windows() {
        let a = run(
            Os::Windows,
            &[
                ("APPDATA", r"C:\Users\a\AppData\Roaming"),
                ("LOCALAPPDATA", r"C:\Users\a\AppData\Local"),
            ],
            Some(r"C:\Games\CityBuilder"),
            None,
        );
        assert_eq!(a.mode, PathMode::Standard);
        assert_eq!(
            a.settings_file,
            p(r"C:\Users\a\AppData\Roaming")
                .join("CityBuilder")
                .join("settings.ron")
        );
        assert_eq!(
            a.saves_dir,
            p(r"C:\Users\a\AppData\Roaming")
                .join("CityBuilder")
                .join("saves")
        );
        assert_eq!(
            a.logs_dir,
            p(r"C:\Users\a\AppData\Local")
                .join("CityBuilder")
                .join("logs")
        );
        assert_eq!(
            a.crashes_dir,
            p(r"C:\Users\a\AppData\Local")
                .join("CityBuilder")
                .join("crashes")
        );
        // Missing APPDATA falls back to USERPROFILE.
        let b = run(Os::Windows, &[("USERPROFILE", r"C:\Users\b")], None, None);
        assert_eq!(
            b.saves_dir,
            p(r"C:\Users\b")
                .join("AppData")
                .join("Roaming")
                .join("CityBuilder")
                .join("saves")
        );
        assert_eq!(
            b.logs_dir,
            p(r"C:\Users\b")
                .join("AppData")
                .join("Local")
                .join("CityBuilder")
                .join("logs")
        );
        let c = run(Os::Windows, &[], None, None);
        assert_eq!(c.mode, PathMode::TempFallback);
    }

    #[test]
    fn paths_resolve_linux() {
        let a = run(Os::Linux, &[("HOME", "/home/a")], None, None);
        assert_eq!(
            a.settings_file,
            p("/home/a/.config/city-builder/settings.ron")
        );
        assert_eq!(a.saves_dir, p("/home/a/.local/share/city-builder/saves"));
        assert_eq!(a.logs_dir, p("/home/a/.local/state/city-builder/logs"));
        assert_eq!(
            a.crashes_dir,
            p("/home/a/.local/state/city-builder/crashes")
        );
        assert_eq!(a.cache_dir, p("/home/a/.cache/city-builder"));
        let b = run(
            Os::Linux,
            &[
                ("HOME", "/home/a"),
                ("XDG_CONFIG_HOME", "/cfg"),
                ("XDG_STATE_HOME", "relative/state"),
                ("XDG_DATA_HOME", ""),
            ],
            None,
            None,
        );
        assert_eq!(b.settings_file, p("/cfg/city-builder/settings.ron"));
        assert_eq!(
            b.logs_dir,
            p("/home/a/.local/state/city-builder/logs"),
            "relative XDG ignored"
        );
        assert_eq!(
            b.saves_dir,
            p("/home/a/.local/share/city-builder/saves"),
            "empty XDG ignored"
        );
        assert_eq!(run(Os::Linux, &[], None, None).mode, PathMode::TempFallback);
    }

    #[test]
    fn paths_resolve_macos() {
        let a = run(
            Os::MacOs,
            &[("HOME", "/Users/a")],
            Some("/Applications/CityBuilder.app/Contents/MacOS"),
            None,
        );
        assert_eq!(
            a.settings_file,
            p("/Users/a/Library/Application Support/CityBuilder/settings.ron")
        );
        assert_eq!(
            a.saves_dir,
            p("/Users/a/Library/Application Support/CityBuilder/saves")
        );
        assert_eq!(a.logs_dir, p("/Users/a/Library/Logs/CityBuilder"));
        assert_eq!(
            a.crashes_dir,
            p("/Users/a/Library/Logs/CityBuilder/crashes")
        );
        assert_eq!(a.cache_dir, p("/Users/a/Library/Caches/CityBuilder"));
    }

    #[test]
    fn paths_portable_mode() {
        let a = run(
            Os::Linux,
            &[("HOME", "/home/a")],
            Some("/opt/cb"),
            Some("/opt/cb/portable.txt"),
        );
        assert_eq!(a.mode, PathMode::Portable);
        assert_eq!(a.settings_file, p("/opt/cb/userdata/settings.ron"));
        assert_eq!(a.logs_dir, p("/opt/cb/userdata/logs"));
        // macOS: portable.txt sits next to the .app bundle.
        let m = run(
            Os::MacOs,
            &[("HOME", "/Users/a")],
            Some("/Volumes/USB/CityBuilder.app/Contents/MacOS"),
            Some("/Volumes/USB/portable.txt"),
        );
        assert_eq!(m.mode, PathMode::Portable);
        assert_eq!(m.saves_dir, p("/Volumes/USB/userdata/saves"));
        // Without the marker: standard.
        let s = run(Os::Linux, &[("HOME", "/home/a")], Some("/opt/cb"), None);
        assert_eq!(s.mode, PathMode::Standard);
    }

    #[test]
    fn paths_cb_userdata_override() {
        let a = run(
            Os::Windows,
            &[("CB_USERDATA", r"D:\cbdata"), ("APPDATA", r"C:\x")],
            Some(r"C:\g"),
            Some(r"C:\g\portable.txt"),
        );
        assert_eq!(a.mode, PathMode::Override);
        assert_eq!(a.saves_dir, p(r"D:\cbdata").join("saves"));
        assert_eq!(a.crashes_dir, p(r"D:\cbdata").join("crashes"));
    }

    #[test]
    fn paths_app_translocation_ignores_portable() {
        let a = run(
            Os::MacOs,
            &[("HOME", "/Users/a")],
            Some(
                "/private/var/folders/xy/T/AppTranslocation/1234/d/CityBuilder.app/Contents/MacOS",
            ),
            Some("/private/var/folders/xy/T/AppTranslocation/1234/d/portable.txt"),
        );
        assert_eq!(a.mode, PathMode::Standard);
        assert_eq!(
            a.saves_dir,
            p("/Users/a/Library/Application Support/CityBuilder/saves")
        );
    }
}
