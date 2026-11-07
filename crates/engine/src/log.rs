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
}

impl LogLine {
    /// Search progress, which vastly outnumbers everything else.
    pub fn is_info(&self) -> bool {
        self.kind == LogKind::Received && self.text.starts_with("info")
    }
}

/// Shared handle to one engine's log. Outlives the process, so a crash can be inspected.
#[derive(Clone, Default)]
pub struct EngineLog(Arc<Mutex<VecDeque<LogLine>>>);

impl EngineLog {
    pub fn push(&self, kind: LogKind, text: impl Into<String>) {
        let Ok(mut lines) = self.0.lock() else { return };
        if lines.len() == CAPACITY {
            lines.pop_front();
        }
        lines.push_back(LogLine {
            kind,
            text: text.into(),
        });
    }

    pub fn snapshot(&self, include_info: bool) -> Vec<LogLine> {
        let Ok(lines) = self.0.lock() else {
            return Vec::new();
        };
        lines
            .iter()
            .filter(|line| include_info || !line.is_info())
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        if let Ok(mut lines) = self.0.lock() {
            lines.clear();
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
        log.push(LogKind::Received, "bestmove e2e4");
        assert_eq!(log.snapshot(true).len(), 3);
        assert_eq!(log.snapshot(false).len(), 2);
        for index in 0..CAPACITY + 10 {
            log.push(LogKind::Sent, index.to_string());
        }
        let lines = log.snapshot(true);
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines.last().unwrap().text, (CAPACITY + 9).to_string());
        log.clear();
        assert!(log.snapshot(true).is_empty());
    }
}
