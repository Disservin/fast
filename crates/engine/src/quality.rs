//! How much worse a candidate is than the best line, for colouring MultiPV arrows.

use crate::Score;

/// Candidate quality relative to the best line, Lichess-style thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
}

/// Winning chances in [-1, 1] for the side the score belongs to (Lichess' curve).
pub fn winning_chances(score: Score) -> f64 {
    let cp = match score {
        Score::Cp(cp) => f64::from(cp.clamp(-1000, 1000)),
        Score::Mate(mate) if mate > 0 => 1000.0,
        Score::Mate(_) => -1000.0,
    };
    2.0 / (1.0 + (-0.003_682_08 * cp).exp()) - 1.0
}

/// Grades `score` against `best`; both are from the side to move's point of view.
pub fn classify(best: Score, score: Score) -> Quality {
    if score.sort_key() >= best.sort_key() {
        return Quality::Best;
    }
    let drop = winning_chances(best) - winning_chances(score);
    match drop {
        d if d < 0.025 => Quality::Best,
        d if d < 0.1 => Quality::Good,
        d if d < 0.2 => Quality::Inaccuracy,
        d if d < 0.3 => Quality::Mistake,
        _ => Quality::Blunder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_by_lost_winning_chances() {
        assert_eq!(classify(Score::Cp(40), Score::Cp(40)), Quality::Best);
        assert_eq!(classify(Score::Cp(40), Score::Cp(35)), Quality::Best);
        assert_eq!(classify(Score::Cp(40), Score::Cp(0)), Quality::Good);
        assert_eq!(classify(Score::Cp(40), Score::Cp(-30)), Quality::Inaccuracy);
        assert_eq!(classify(Score::Cp(40), Score::Cp(-80)), Quality::Mistake);
        assert_eq!(classify(Score::Cp(40), Score::Cp(-200)), Quality::Blunder);
        assert_eq!(classify(Score::Mate(3), Score::Cp(-600)), Quality::Blunder);
        // In a won position small differences barely matter.
        assert_eq!(classify(Score::Cp(900), Score::Cp(700)), Quality::Good);
    }

    #[test]
    fn chances_are_symmetric_and_bounded() {
        assert!((winning_chances(Score::Cp(0))).abs() < 1e-9);
        let up = winning_chances(Score::Cp(300));
        assert!((up + winning_chances(Score::Cp(-300))).abs() < 1e-9);
        assert!(winning_chances(Score::Mate(1)) < 1.0);
    }
}
