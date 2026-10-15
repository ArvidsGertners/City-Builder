//! Panic hook and crash reports (ADR-0022, amended: the hook only uses `try_lock`).
//!
//! The report is plain text with fenced sections so players can paste it anywhere. The hook
//! is chained to the default hook, writes `crashes/crash-YYYYMMDD-HHMMSS.txt` and a
//! `.last_crash` marker that the next start turns into a notice.

use std::backtrace::Backtrace;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::logging::LogRing;
use crate::platform::Os;

/// Log lines included in a report.
pub const REPORT_LOG_LINES: usize = 200;
/// Marker file name inside the crashes directory.
pub const LAST_CRASH_MARKER: &str = ".last_crash";

struct CrashState {
    crashes_dir: PathBuf,
    ring: LogRing,
    adapter: Mutex<String>,
    settings: Mutex<String>,
    sim: Mutex<String>,
}

static STATE: OnceLock<CrashState> = OnceLock::new();

fn set(slot: impl Fn(&CrashState) -> &Mutex<String>, value: String) {
    if let Some(state) = STATE.get() {
        // Never block a frame on the hook; a missed update is harmless.
        if let Ok(mut s) = slot(state).try_lock() {
            *s = value;
        }
    }
}

/// Records the GPU adapter line (name, backend, driver) once known.
pub fn set_adapter_info(info: String) {
    set(|s| &s.adapter, info);
}

/// Records a short settings summary.
pub fn set_settings_summary(summary: String) {
    set(|s| &s.settings, summary);
}

/// Records a short sim summary (seed, tick, game time, population, last commands).
pub fn set_sim_summary(summary: String) {
    set(|s| &s.sim, summary);
}

fn read(m: &Mutex<String>) -> String {
    match m.try_lock() {
        Ok(s) if s.is_empty() => "(not yet known)".to_owned(),
        Ok(s) => s.clone(),
        Err(_) => "(unavailable: lock busy)".to_owned(),
    }
}

/// Everything a report contains.
#[derive(Clone, Debug, Default)]
pub struct CrashInput {
    /// UTC timestamp `YYYY-MM-DD HH:MM:SS`.
    pub timestamp: String,
    /// Panic message.
    pub message: String,
    /// `file:line:column`.
    pub location: String,
    /// Thread name.
    pub thread: String,
    /// Captured backtrace.
    pub backtrace: String,
    /// Game version.
    pub version: String,
    /// Git hash.
    pub git_hash: String,
    /// Build profile.
    pub profile: String,
    /// OS and architecture.
    pub os_arch: String,
    /// GPU adapter line.
    pub adapter: String,
    /// Settings summary.
    pub settings: String,
    /// Sim summary.
    pub sim: String,
    /// Recent log lines.
    pub log_lines: Vec<String>,
}

/// Section titles in report order (also asserted by the crash test).
pub const SECTIONS: [&str; 10] = [
    "Summary",
    "Panic",
    "Thread",
    "Backtrace",
    "Build",
    "System",
    "GPU adapter",
    "Settings",
    "Simulation",
    "Last log lines",
];

/// Formats the report.
#[must_use]
pub fn build_report(c: &CrashInput) -> String {
    let fence = |title: &str, body: &str| format!("## {title}\n```\n{}\n```\n\n", body.trim_end());
    let mut out = String::from("# City Builder crash report\n\n");
    out += &fence(
        SECTIONS[0],
        &format!(
            "City Builder {} ({}) crashed at {} UTC: {}",
            c.version, c.git_hash, c.timestamp, c.message
        ),
    );
    out += &fence(SECTIONS[1], &format!("{}\nat {}", c.message, c.location));
    out += &fence(SECTIONS[2], &c.thread);
    out += &fence(SECTIONS[3], &c.backtrace);
    out += &fence(
        SECTIONS[4],
        &format!(
            "version {}\ngit {}\nprofile {}",
            c.version, c.git_hash, c.profile
        ),
    );
    out += &fence(SECTIONS[5], &c.os_arch);
    out += &fence(SECTIONS[6], &c.adapter);
    out += &fence(SECTIONS[7], &c.settings);
    out += &fence(SECTIONS[8], &c.sim);
    out += &fence(SECTIONS[9], &c.log_lines.join("\n"));
    out
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's civil-from-days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// UTC `(YYYY-MM-DD HH:MM:SS, YYYYMMDD-HHMMSS)` for `unix_secs`.
#[must_use]
pub fn utc_stamps(unix_secs: i64) -> (String, String) {
    let (y, mo, d) = civil_from_days(unix_secs.div_euclid(86_400));
    let s = unix_secs.rem_euclid(86_400);
    let (h, mi, se) = (s / 3600, (s % 3600) / 60, s % 60);
    (
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{se:02}"),
        format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{se:02}"),
    )
}

/// Installs the hook. Call once, after logging, before Bevy starts.
pub fn install(crashes_dir: &Path, ring: LogRing) {
    let _ = STATE.set(CrashState {
        crashes_dir: crashes_dir.to_path_buf(),
        ring,
        adapter: Mutex::new(String::new()),
        settings: Mutex::new(String::new()),
        sim: Mutex::new(String::new()),
    });
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(non-string panic payload)".to_owned());
        let location = info
            .location()
            .map_or_else(|| "(unknown)".to_owned(), ToString::to_string);
        match write_report(message, location) {
            Ok(path) => eprintln!("City Builder crashed. Report: {}", path.display()),
            Err(e) => eprintln!("City Builder crashed; writing the report failed: {e}"),
        }
        default_hook(info);
    }));
}

fn write_report(message: String, location: String) -> std::io::Result<PathBuf> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let (timestamp, file_stamp) = utc_stamps(secs);
    let state = STATE.get();
    let thread = std::thread::current();
    let input = CrashInput {
        timestamp,
        message,
        location,
        thread: thread.name().unwrap_or("(unnamed)").to_owned(),
        backtrace: Backtrace::force_capture().to_string(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        git_hash: env!("CB_GIT_HASH").to_owned(),
        profile: env!("CB_BUILD_PROFILE").to_owned(),
        os_arch: format!("{} {}", Os::current().name(), std::env::consts::ARCH),
        adapter: state.map_or_else(String::new, |s| read(&s.adapter)),
        settings: state.map_or_else(String::new, |s| read(&s.settings)),
        sim: state.map_or_else(String::new, |s| read(&s.sim)),
        log_lines: state
            .and_then(|s| s.ring.try_tail(REPORT_LOG_LINES))
            .unwrap_or_else(|| vec!["(log buffer busy)".to_owned()]),
    };
    let report = build_report(&input);
    let dir = state.map_or_else(std::env::temp_dir, |s| s.crashes_dir.clone());
    let path = write_into(&dir, &file_stamp, &report)
        .or_else(|_| write_into(&std::env::temp_dir(), &file_stamp, &report))?;
    let _ = std::fs::write(
        dir.join(LAST_CRASH_MARKER),
        path.to_string_lossy().as_bytes(),
    );
    Ok(path)
}

fn write_into(dir: &Path, stamp: &str, report: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("crash-{stamp}.txt"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("crash-{stamp}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, report)?;
    Ok(path)
}

/// The report from the previous session, if it crashed (marker present and report readable).
#[must_use]
pub fn last_crash(crashes_dir: &Path) -> Option<(PathBuf, String)> {
    let marker = std::fs::read_to_string(crashes_dir.join(LAST_CRASH_MARKER)).ok()?;
    let path = PathBuf::from(marker.trim());
    let text = std::fs::read_to_string(&path).ok()?;
    Some((path, text))
}

/// Removes the marker after the player dismissed the notice.
pub fn dismiss_last_crash(crashes_dir: &Path) {
    let _ = std::fs::remove_file(crashes_dir.join(LAST_CRASH_MARKER));
}

/// One-line summary used in logs and the settings section.
#[must_use]
pub fn summary_line(parts: &[(&str, String)]) -> String {
    let mut s = String::new();
    for (k, v) in parts {
        let _ = write!(s, "{k}={v} ");
    }
    s.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamps_known_values() {
        assert_eq!(utc_stamps(0).0, "1970-01-01 00:00:00");
        assert_eq!(utc_stamps(1_790_208_000).1, "20260924-000000");
        assert_eq!(utc_stamps(951_825_600).0, "2000-02-29 12:00:00");
    }

    #[test]
    fn report_has_sections_in_order() {
        let r = build_report(&CrashInput {
            message: "boom".into(),
            ..CrashInput::default()
        });
        let mut last = 0;
        for title in SECTIONS {
            let pos = r.find(&format!("## {title}\n")).unwrap();
            assert!(pos >= last, "{title} out of order");
            last = pos;
        }
        assert!(r.contains("boom"));
    }
}
