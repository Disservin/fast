//! Attempt evidence and exactly-once review submission.
use crate::{Card, Grade};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attempt {
    pub hints: u32,
    pub reveals: u32,
    pub failures: u32,
    pub elapsed_ms: u64,
}
impl Attempt {
    pub fn scheduling_grade(&self, requested: Grade) -> Grade {
        if self.failures > 0 || self.reveals > 0 {
            Grade::Again
        } else if self.hints > 0 && matches!(requested, Grade::Good | Grade::Easy) {
            Grade::Hard
        } else {
            requested
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewEvent {
    pub id: u64,
    pub day: i64,
    pub position: String,
    pub answer: String,
    pub attempt: Attempt,
    pub requested: Grade,
    pub applied: Grade,
    pub before: Card,
    pub after: Card,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_cannot_erase_failure_or_help() {
        let mut attempt = Attempt {
            failures: 1,
            ..Default::default()
        };
        assert_eq!(attempt.scheduling_grade(Grade::Easy), Grade::Again);
        attempt.failures = 0;
        attempt.hints = 1;
        assert_eq!(attempt.scheduling_grade(Grade::Good), Grade::Hard);
        attempt.reveals = 1;
        assert_eq!(attempt.scheduling_grade(Grade::Good), Grade::Again);
    }
}

/// Queue, evidence and counters commit alongside cards and review history.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StudySession {
    pub saved_at: u64,
    pub queue: Vec<crate::Drill>,
    pub finished: usize,
    pub total: usize,
    pub every_reply: bool,
    pub attempts: std::collections::HashMap<String, Attempt>,
    pub phase: u8,
}
