//! Crash reporter end to end: run the real binary with `--debug-panic` (PROGRESS M1).

use std::process::Command;

use cb_game::bootstrap::crash::{SECTIONS, last_crash};

#[test]
fn crash_report_contains_all_sections() {
    let root = std::env::temp_dir().join(format!("cb-crash-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let out = Command::new(env!("CARGO_BIN_EXE_cb_game"))
        .arg("--debug-panic")
        .env("CB_USERDATA", &root)
        .env("RUST_BACKTRACE", "0")
        .output()
        .unwrap();
    assert!(!out.status.success(), "a panic must exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Report:"), "stderr: {stderr}");

    let crashes = root.join("crashes");
    let (path, report) = last_crash(&crashes).expect("marker and report");
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("crash-")
    );
    for title in SECTIONS {
        assert!(
            report.contains(&format!("## {title}\n")),
            "missing section {title}"
        );
    }
    for needle in [
        "debug panic requested with --debug-panic",
        "lib.rs:", // panic location; the separator before it is `\\` on Windows
        env!("CARGO_PKG_VERSION"),
        "lang=",
        "City Builder", // version line from the log ring
    ] {
        assert!(
            report.contains(needle),
            "report lacks {needle:?}:\n{report}"
        );
    }
    // Logging went to the same userdata root.
    assert!(root.join("logs").is_dir());
    // Default settings were written on first start.
    assert!(root.join("settings.ron").is_file());
    let _ = std::fs::remove_dir_all(&root);
}
