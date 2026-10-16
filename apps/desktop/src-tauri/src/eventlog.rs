//! In-memory event log the UI polls for its console pane.
//!
//! Entries are monotonically numbered so the frontend can ask for
//! "everything after id N". Messages are capped in length; callers are
//! responsible for redacting secrets before pushing (helpers exist in
//! `kvn_core::secret`).
//!
//! The same entries are appended (best-effort) to a bounded file —
//! `logs/kvn.log` — so that a crash or a failed connection can be diagnosed
//! after the fact even though the console pane itself is in-memory.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const MAX_MESSAGE_LEN: usize = 2000;
/// `kvn.log` is rotated once past this size (one `.log.1` file is kept).
const MAX_LOG_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub ts_ms: u64,
    pub level: String,
    pub source: String,
    pub message: String,
}

pub struct EventLog {
    entries: Mutex<VecDeque<LogEntry>>,
    next_id: AtomicU64,
    capacity: usize,
    sink: Option<Mutex<LogSink>>,
}

struct LogSink {
    path: PathBuf,
    file: File,
    bytes: u64,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        EventLog {
            entries: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            capacity: capacity.max(64),
            sink: None,
        }
    }

    /// In-memory log plus a bounded append-only file at `path`.
    pub fn with_file(capacity: usize, path: PathBuf) -> Self {
        let sink = open_sink(&path).map(|(file, bytes)| Mutex::new(LogSink { path, file, bytes }));
        EventLog {
            entries: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            capacity: capacity.max(64),
            sink,
        }
    }

    pub fn push(&self, level: &str, source: &str, message: impl AsRef<str>) -> LogEntry {
        let mut message = message.as_ref().to_owned();
        if message.len() > MAX_MESSAGE_LEN {
            let mut cut = MAX_MESSAGE_LEN;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
            message.push('…');
        }
        let entry = LogEntry {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            ts_ms: now_ms(),
            level: level.to_owned(),
            source: source.to_owned(),
            message,
        };
        {
            let mut entries = self.entries.lock().unwrap();
            if entries.len() >= self.capacity {
                entries.pop_front();
            }
            entries.push_back(entry.clone());
        }
        if let Some(sink) = &self.sink {
            write_to_file(sink, &entry);
        }
        entry
    }

    pub fn since(&self, after_id: u64) -> Vec<LogEntry> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.id > after_id)
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    pub fn tail(&self, count: usize) -> Vec<LogEntry> {
        let entries = self.entries.lock().unwrap();
        entries.iter().rev().take(count).rev().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

fn open_sink(path: &Path) -> Option<(File, u64)> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some((file, bytes))
}

/// Append one entry to the log file, rotating it once past the size cap.
/// All failures are silent by design: the console pane must never break
/// because a log file could not be written.
fn write_to_file(sink: &Mutex<LogSink>, entry: &LogEntry) {
    let Ok(mut sink) = sink.lock() else {
        return;
    };
    if sink.bytes > MAX_LOG_FILE_BYTES {
        let rotated = sink.path.with_extension("log.1");
        let _ = std::fs::remove_file(&rotated);
        if std::fs::rename(&sink.path, &rotated).is_ok() {
            if let Some((file, _)) = open_sink(&sink.path) {
                sink.file = file;
                sink.bytes = 0;
            }
        }
    }
    let line = format!(
        "{} {:5} {:<12} {}\n",
        iso_utc(entry.ts_ms),
        entry.level,
        entry.source,
        entry.message
    );
    if sink.file.write_all(line.as_bytes()).is_ok() && sink.file.flush().is_ok() {
        sink.bytes += line.len() as u64;
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Format epoch milliseconds as `YYYY-MM-DDTHH:MM:SSZ` without extra deps.
pub fn iso_utc(epoch_ms: u64) -> String {
    let secs = (epoch_ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic_and_since_filters() {
        let log = EventLog::new(16);
        let a = log.push("info", "ui", "one");
        let b = log.push("warn", "engine", "two");
        assert!(b.id > a.id);
        let all = log.since(0);
        assert_eq!(all.len(), 2);
        let newer = log.since(a.id);
        assert_eq!(newer.len(), 1);
        assert_eq!(newer[0].message, "two");
    }

    #[test]
    fn capacity_trims_old_entries() {
        // Capacity is clamped to a sane minimum (64); the ring must not grow
        // past it.
        let log = EventLog::new(4);
        for _ in 0..100 {
            log.push("debug", "test", "x");
        }
        assert_eq!(log.len(), 64);
    }

    #[test]
    fn long_messages_are_capped_on_char_boundaries() {
        let log = EventLog::new(4);
        let entry = log.push("error", "test", "ы".repeat(5000));
        assert!(entry.message.len() <= MAX_MESSAGE_LEN + 4);
    }

    #[test]
    fn file_sink_writes_and_rotates() {
        let dir = std::env::temp_dir().join(format!("kvn-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kvn.log");
        let log = EventLog::with_file(64, path.clone());
        log.push("info", "test", "hello");
        log.push("warn", "test", "world");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("hello") && text.contains("world"));

        // Force the rotation threshold without writing megabytes.
        {
            let sink = log.sink.as_ref().unwrap();
            let mut sink = sink.lock().unwrap();
            sink.bytes = MAX_LOG_FILE_BYTES + 1;
        }
        log.push("info", "test", "after-rotation");
        let rotated = path.with_extension("log.1");
        assert!(rotated.exists(), "old log must be kept as .log.1");
        let fresh = std::fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("after-rotation"));
        assert!(!fresh.contains("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn iso_utc_formats_known_instants() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        // 2026-03-27T12:34:56Z
        assert_eq!(iso_utc(1_774_614_896_000), "2026-03-27T12:34:56Z");
        // leap day
        assert_eq!(iso_utc(1_709_164_800_000), "2024-02-29T00:00:00Z");
    }
}
