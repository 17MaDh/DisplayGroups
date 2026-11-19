//! Rotating diagnostic logs in `%LOCALAPPDATA%\DisplayGroups\logs` (7 daily files).
//! `DISPLAYGROUPS_LOG=debug` overrides the configured level.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

/// Written next to the logs when logging cannot start, so a silent log can be explained.
const FAILURE_NOTE: &str = "logging-failed.txt";

/// Keep the returned guard alive until exit so buffered lines are flushed.
///
/// A long-running agent must not stay without a log because the file was briefly
/// unavailable when it started, so a failure is retried once; if it still fails, the
/// reason is noted in `logging-failed.txt`.
pub fn init(dir: &Path, level: &str) -> Option<WorkerGuard> {
    let first = match try_init(dir, level) {
        Ok(guard) => return Some(guard),
        Err(e) => e,
    };
    std::thread::sleep(Duration::from_millis(500));
    match try_init(dir, level) {
        Ok(guard) => {
            tracing::warn!("logging started on the second try (first: {first})");
            Some(guard)
        }
        Err(second) => {
            note_failure(dir, &format!("{first}; retried: {second}"));
            None
        }
    }
}

fn try_init(dir: &Path, level: &str) -> Result<WorkerGuard, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("displaygroups")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .map_err(|e| format!("log file: {e}"))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_env("DISPLAYGROUPS_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init()
        .map_err(|e| format!("subscriber: {e}"))?;
    Ok(guard)
}

fn note_failure(dir: &Path, reason: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = format!(
        "unix time {seconds}, process {} ({}): logging did not start: {reason}\r\n",
        std::process::id(),
        args.join(" ")
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(FAILURE_NOTE))
        .and_then(|mut f| f.write_all(line.as_bytes()));
}
