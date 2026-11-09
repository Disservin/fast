//! A bounded record of what passed between the app and an engine process.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// Oldest lines are dropped past this count, so long sessions stay bounded.
const CAPACITY: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    /// Written to the engine's stdin.
    Sent,
    /// Read from the engine's stdout.
    Received,
    /// Read from the engine's stderr.
    Stderr,
    /// Noted by the app, such as a failure to start.
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
    /// Position in the whole log, counting lines already dropped, so a view can keep
    /// its place while old lines fall off the front.
    pub seq: u64,
}

impl LogLine {
    /// Search progress, which vastly outnumbers everything else. `info string` is a
    /// message from the engine, not progress, so it is never filtered out.
    pub fn is_info(&self) -> bool {
        self.kind == LogKind::Received
            && self.text.starts_with("info")
            && !self.text.starts_with("info string")
    }
}

#[derive(Default)]
struct Inner {
    lines: VecDeque<LogLine>,
    /// Bumped on every change, so readers can tell when to take a new snapshot.
    revision: u64,
    /// The sequence number the next line gets.
    next_seq: u64,
}

/// Shared handle to one engine's log. Outlives the process, so a crash can be inspected.
#[derive(Clone, Default)]
pub struct EngineLog(Arc<Mutex<Inner>>);

impl EngineLog {
    pub fn push(&self, kind: LogKind, text: impl Into<String>) {
        let Ok(mut inner) = self.0.lock() else { return };
        if inner.lines.len() == CAPACITY {
            inner.lines.pop_front();
        }
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.lines.push_back(LogLine {
            kind,
            text: text.into(),
            seq,
        });
        inner.revision += 1;
    }

    /// Changes whenever a line is added or the log is cleared.
    pub fn revision(&self) -> u64 {
        self.0.lock().map_or(0, |inner| inner.revision)
    }

    pub fn snapshot(&self, include_info: bool) -> Vec<LogLine> {
        let Ok(inner) = self.0.lock() else {
            return Vec::new();
        };
        inner
            .lines
            .iter()
            .filter(|line| include_info || !line.is_info())
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        if let Ok(mut inner) = self.0.lock() {
            inner.lines.clear();
            inner.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_info_and_stays_bounded() {
        let log = EngineLog::default();
        log.push(LogKind::Sent, "go infinite");
        log.push(LogKind::Received, "info depth 1 pv e2e4");
        log.push(LogKind::Received, "info string NNUE evaluation enabled");
        log.push(LogKind::Received, "bestmove e2e4");
        assert_eq!(log.snapshot(true).len(), 4);
        assert_eq!(log.snapshot(false).len(), 3);
        let before = log.revision();
        for index in 0..CAPACITY + 10 {
            log.push(LogKind::Sent, index.to_string());
        }
        let lines = log.snapshot(true);
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines.last().unwrap().text, (CAPACITY + 9).to_string());
        assert!(log.revision() > before);
        let filled = log.revision();
        log.clear();
        assert!(log.snapshot(true).is_empty());
        assert!(log.revision() > filled);
        log.push(LogKind::Sent, "uci");
        assert!(log.snapshot(true)[0].seq > lines.last().unwrap().seq);
    }
}
