//! Integration test: log lines reach the file in `AppPaths::logs_dir` (own process, so the
//! global subscriber is ours).

use cb_game::bootstrap::logging;

#[test]
fn log_line_written_to_app_paths_logs() {
    let root = std::env::temp_dir().join(format!("cb-log-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let logs = root.join("logs");
    let handles = logging::init(&logs, "info");
    tracing::info!(marker = 7, "integration-log-marker");
    assert!(
        handles
            .ring
            .tail(10)
            .iter()
            .any(|l| l.contains("integration-log-marker"))
    );
    drop(handles); // flushes the non-blocking writer
    let text: String = std::fs::read_dir(&logs)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("city-builder"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(
        text.contains("integration-log-marker"),
        "log file content: {text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
