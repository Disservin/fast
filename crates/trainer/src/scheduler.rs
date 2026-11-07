//! SM-2 style scheduling. Pure and deterministic: time is passed in as a day number.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    Again,
    Hard,
    Good,
    Easy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardState {
    #[default]
    New,
    Learning,
    Review,
}

/// Scheduling data for one repertoire move.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub state: CardState,
    pub ease: f32,
    pub interval_days: u32,
    /// Day number the card is next due on.
    pub due_day: i64,
    pub lapses: u32,
}

impl Default for Card {
    fn default() -> Self {
        Self {
            state: CardState::New,
            ease: 2.5,
            interval_days: 0,
            due_day: 0,
            lapses: 0,
        }
    }
}

/// The interval each grade would produce, shown on the recall buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intervals {
    pub again: u32,
    pub hard: u32,
    pub good: u32,
    pub easy: u32,
}

const MIN_EASE: f32 = 1.3;
const MAX_EASE: f32 = 3.0;

impl Card {
    pub fn is_due(&self, today: i64) -> bool {
        self.state != CardState::New && self.due_day <= today
    }

    pub fn intervals(&self) -> Intervals {
        Intervals {
            again: 0,
            hard: self.interval_for(Grade::Hard),
            good: self.interval_for(Grade::Good),
            easy: self.interval_for(Grade::Easy),
        }
    }

    fn interval_for(&self, grade: Grade) -> u32 {
        let previous = self.interval_days as f32;
        let days = if self.state == CardState::Review && previous >= 1.0 {
            match grade {
                Grade::Again => 0.0,
                Grade::Hard => (previous * 1.2).max(previous + 1.0),
                Grade::Good => (previous * self.ease).max(previous + 2.0),
                Grade::Easy => (previous * self.ease * 1.3).max(previous + 4.0),
            }
        } else {
            // First steps: a short ladder that gets the user reviewing quickly.
            match grade {
                Grade::Again => 0.0,
                Grade::Hard => 1.0,
                Grade::Good => 4.0,
                Grade::Easy => 11.0,
            }
        };
        days.round() as u32
    }

    /// Records an answer, returning the updated card. "Again" is due today.
    pub fn answered(mut self, grade: Grade, today: i64) -> Self {
        let interval = self.interval_for(grade);
        let ease_delta = match grade {
            Grade::Again => -0.2,
            Grade::Hard => -0.15,
            Grade::Good => 0.0,
            Grade::Easy => 0.15,
        };
        self.ease = (self.ease + ease_delta).clamp(MIN_EASE, MAX_EASE);
        self.interval_days = interval;
        self.due_day = today + i64::from(interval);
        match grade {
            Grade::Again => {
                self.lapses += 1;
                self.state = CardState::Learning;
            }
            _ => self.state = CardState::Review,
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_card_offers_the_first_step_ladder() {
        let intervals = Card::default().intervals();
        assert_eq!(
            intervals,
            Intervals {
                again: 0,
                hard: 1,
                good: 4,
                easy: 11
            }
        );
    }

    #[test]
    fn good_answers_grow_deterministically() {
        let mut card = Card::default();
        let mut today = 100;
        let mut history = Vec::new();
        for _ in 0..4 {
            card = card.answered(Grade::Good, today);
            history.push(card.interval_days);
            today = card.due_day;
        }
        assert_eq!(history, vec![4, 10, 25, 63]);
        assert_eq!(card.due_day, today);
        assert!(card.is_due(today));
        assert!(!card.is_due(today - 1));
    }

    #[test]
    fn again_resets_interval_and_lowers_ease() {
        let card = Card::default()
            .answered(Grade::Good, 0)
            .answered(Grade::Again, 4);
        assert_eq!(card.interval_days, 0);
        assert_eq!(card.state, CardState::Learning);
        assert_eq!(card.lapses, 1);
        assert!(card.ease < 2.5);
        assert!(card.is_due(4));
        // Back on the first-step ladder afterwards.
        assert_eq!(card.intervals().good, 4);
    }

    #[test]
    fn ease_stays_in_bounds() {
        let mut low = Card::default();
        let mut high = Card::default();
        for _ in 0..50 {
            low = low.answered(Grade::Hard, 0);
            high = high.answered(Grade::Easy, 0);
            high.interval_days = 1;
        }
        assert!((low.ease - MIN_EASE).abs() < f32::EPSILON);
        assert!((high.ease - MAX_EASE).abs() < f32::EPSILON);
    }

    #[test]
    fn hard_never_shrinks_a_review_interval() {
        let card = Card {
            state: CardState::Review,
            interval_days: 10,
            ..Card::default()
        };
        assert!(card.intervals().hard > 10);
        assert!(card.intervals().good > card.intervals().hard);
        assert!(card.intervals().easy > card.intervals().good);
    }

    #[test]
    fn new_cards_are_never_due() {
        assert!(!Card::default().is_due(1_000));
    }
}
