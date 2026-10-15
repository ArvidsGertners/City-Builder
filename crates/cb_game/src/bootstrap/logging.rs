//! Logging (ADR-0022): a global `tracing` subscriber installed in `main()` before Bevy starts,
//! so bootstrap messages are captured too. Layers: daily rolling file in `AppPaths::logs_dir`,
//! stderr, an in-memory ring buffer (crash reports, in-game console) and an error counter
//! (smoke test). Bevy's own `LogPlugin` is not used.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

/// Lines kept in memory.
pub const RING_CAPACITY: usize = 500;
/// Default filter when neither `--log`, `CB_LOG` nor settings give one.
pub const DEFAULT_FILTER: &str = "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn";

/// The last [`RING_CAPACITY`] formatted log lines.
#[derive(Clone, Default, Debug)]
pub struct LogRing {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl LogRing {
    /// Appends a line, dropping the oldest beyond capacity.
    pub fn push(&self, line: String) {
        // A poisoned lock still holds valid lines; keep logging.
        let mut q = match self.inner.lock() {
            Ok(q) => q,
            Err(p) => p.into_inner(),
        };
        if q.len() == RING_CAPACITY {
            q.pop_front();
        }
        q.push_back(line);
    }

    /// The last `n` lines (oldest first). Blocks briefly on the lock.
    #[must_use]
    pub fn tail(&self, n: usize) -> Vec<String> {
        let q = match self.inner.lock() {
            Ok(q) => q,
            Err(p) => p.into_inner(),
        };
        q.iter().skip(q.len().saturating_sub(n)).cloned().collect()
    }

    /// Like [`tail`](Self::tail) but never blocks (for the panic hook); `None` if busy.
    #[must_use]
    pub fn try_tail(&self, n: usize) -> Option<Vec<String>> {
        let q = match self.inner.try_lock() {
            Ok(q) => q,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(q.iter().skip(q.len().saturating_sub(n)).cloned().collect())
    }

    /// Number of stored lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tail(RING_CAPACITY).len()
    }

    /// Whether no line is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Counters for error-level events.
#[derive(Default, Debug)]
pub struct ErrorCounters {
    /// All ERROR events.
    pub errors: AtomicU64,
    /// ERROR events from wgpu/naga or mentioning a validation error.
    pub gpu_validation: AtomicU64,
}

#[derive(Default)]
struct LineVisitor {
    message: String,
    fields: String,
}

impl Visit for LineVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

/// Formats an event as one line: `LEVEL target: message k=v`.
fn format_line(event: &Event<'_>) -> String {
    let mut v = LineVisitor::default();
    event.record(&mut v);
    let meta = event.metadata();
    format!(
        "{:>5} {}: {}{}",
        meta.level(),
        meta.target(),
        v.message,
        v.fields
    )
}

struct RingLayer {
    ring: LogRing,
    counters: Arc<ErrorCounters>,
}

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let line = format_line(event);
        let meta = event.metadata();
        if *meta.level() == Level::ERROR {
            self.counters.errors.fetch_add(1, Ordering::Relaxed);
            let t = meta.target();
            if t.starts_with("wgpu") || t.starts_with("naga") || line.contains("Validation Error") {
                self.counters.gpu_validation.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.ring.push(line);
    }
}

/// Handles kept alive for the process lifetime.
pub struct LogHandles {
    /// Ring buffer of recent lines.
    pub ring: LogRing,
    /// Error counters.
    pub counters: Arc<ErrorCounters>,
    /// Directory of the log files, if file logging works.
    pub log_dir: Option<PathBuf>,
    /// Flushes the non-blocking file writer when dropped.
    pub guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Picks the filter: `--log` > `CB_LOG` > settings > default.
#[must_use]
pub fn choose_filter(cli: Option<&str>, env: Option<&str>, settings: Option<&str>) -> String {
    [cli, env, settings]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or(DEFAULT_FILTER)
        .to_owned()
}

/// Installs the global subscriber. Falls back to stderr only if the log dir is unusable.
pub fn init(logs_dir: &Path, filter: &str) -> LogHandles {
    let ring = LogRing::default();
    let counters = Arc::new(ErrorCounters::default());
    let (env_filter, filter_error) = match EnvFilter::try_new(filter) {
        Ok(f) => (f, None),
        Err(e) => (EnvFilter::new(DEFAULT_FILTER), Some(e.to_string())),
    };

    let file = std::fs::create_dir_all(logs_dir)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("city-builder")
                .filename_suffix("log")
                .max_log_files(7)
                .build(logs_dir)
                .map_err(|e| e.to_string())
        });
    let (file_layer, guard, log_dir, file_error) = match file {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer);
            (Some(layer), Some(guard), Some(logs_dir.to_path_buf()), None)
        }
        Err(e) => (None, None, None, Some(e)),
    };

    let result = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(std::io::stderr().is_terminal())
                .with_writer(std::io::stderr),
        )
        .with(RingLayer {
            ring: ring.clone(),
            counters: Arc::clone(&counters),
        })
        .try_init();
    if let Err(e) = result {
        eprintln!("logging: a global subscriber was already installed: {e}");
    }
    if let Some(e) = filter_error {
        tracing::warn!("invalid log filter {filter:?} ({e}); using {DEFAULT_FILTER:?}");
    }
    if let Some(e) = file_error {
        tracing::warn!("file logging disabled ({}): {e}", logs_dir.display());
    }
    LogHandles {
        ring,
        counters,
        log_dir,
        guard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_last_500() {
        let ring = LogRing::default();
        for i in 0..1234 {
            ring.push(format!("line {i}"));
        }
        assert_eq!(ring.len(), RING_CAPACITY);
        let all = ring.tail(RING_CAPACITY);
        assert_eq!(all.first().map(String::as_str), Some("line 734"));
        assert_eq!(all.last().map(String::as_str), Some("line 1233"));
        let last = ring.try_tail(3).unwrap();
        assert_eq!(last, vec!["line 1231", "line 1232", "line 1233"]);
    }

    #[test]
    fn filter_precedence() {
        assert_eq!(choose_filter(Some("debug"), Some("warn"), None), "debug");
        assert_eq!(choose_filter(Some(" "), Some("warn"), Some("info")), "warn");
        assert_eq!(choose_filter(None, None, None), DEFAULT_FILTER);
    }
}
