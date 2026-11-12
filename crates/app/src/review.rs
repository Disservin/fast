//! Engine review of stored games: accuracy and the moves that lost the most.

use chess_core::{GameTree, Position as _};
use db::GameDetails;

/// Lichess-style accuracy from a win-percentage loss (0..100).
pub fn accuracy_from_loss(average_loss: f64) -> f64 {
    (103.1668 * (-0.04354 * average_loss).exp() - 3.1669).clamp(0.0, 100.0)
}

/// Win percentage from a White-relative centipawn score.
pub(crate) fn win_percent(cp: i32) -> f64 {
    50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * f64::from(cp)).exp()) - 1.0)
}

pub(crate) fn review_accuracy(
    evals: &[Option<i32>],
    white: bool,
    first_white: bool,
) -> Option<f64> {
    if evals.iter().any(Option::is_none) {
        return None;
    }
    let losses: Vec<_> = evals
        .windows(2)
        .enumerate()
        .filter(|(ply, _)| (first_white == (ply % 2 == 0)) == white)
        .map(|(_, pair)| {
            let before = win_percent(pair[0].expect("checked above"));
            let after = win_percent(pair[1].expect("checked above"));
            (if white {
                before - after
            } else {
                after - before
            })
            .max(0.0)
        })
        .collect();
    (!losses.is_empty())
        .then(|| accuracy_from_loss(losses.iter().sum::<f64>() / losses.len() as f64))
}

/// A move from the game review that lost a lot of the engine score, as one line of the
/// preview: the move, what it lost, and a longer explanation for the tooltip.
pub(crate) struct ReviewFinding {
    pub ply: usize,
    /// The move in numbered notation, such as `14.c4` or `14...Nf6`.
    pub label: String,
    /// What was lost, such as `−1.16` or `Mate`.
    pub loss: String,
    pub text: String,
    /// The move-quality NAG for the loss: `?!`, `?` or `??`.
    pub nag: u8,
    /// The engine's line from the position before the move, in SAN.
    pub best: String,
}

/// A review finding as Analyse lists it beside the game.
#[derive(Clone, Debug)]
pub(crate) struct ReviewMark {
    /// The index of the move in the game's mainline.
    pub ply: usize,
    /// The move in numbered notation, such as `14.c4`.
    pub label: String,
    /// What the move lost, such as `−1.16`.
    pub loss: String,
    pub quality: engine::Quality,
    /// What the engine preferred, in numbered SAN.
    pub best: String,
}

/// The review findings of a stored game, for Analyse.
pub(crate) fn review_marks(game: &GameDetails) -> Vec<ReviewMark> {
    review_findings(game)
        .into_iter()
        .map(|finding| ReviewMark {
            ply: finding.ply,
            label: finding.label,
            loss: finding.loss,
            quality: match finding.nag {
                4 => engine::Quality::Blunder,
                2 => engine::Quality::Mistake,
                _ => engine::Quality::Inaccuracy,
            },
            best: finding.best,
        })
        .collect()
}

pub(crate) fn review_findings(game: &GameDetails) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    for ply in 0..game.sans.len() {
        let Some(before) = game
            .review_positions
            .iter()
            .find(|position| position.ply as usize == ply)
        else {
            continue;
        };
        let Some(after) = game
            .review_positions
            .iter()
            .find(|position| position.ply as usize == ply + 1)
        else {
            continue;
        };
        let Ok(before_tree) = GameTree::from_fen(&before.fen) else {
            continue;
        };
        let Ok(after_tree) = GameTree::from_fen(&after.fen) else {
            continue;
        };
        let before_pos = &before_tree.node(GameTree::ROOT).pos;
        let after_pos = &after_tree.node(GameTree::ROOT).pos;
        let white_moved = before_pos.turn().is_white();
        let loss = if white_moved {
            before.eval_cp - after.eval_cp
        } else {
            after.eval_cp - before.eval_cp
        };
        let mate_lost = before.mate_distance.is_some_and(|mate| {
            let favorable_to_mover = (white_moved && mate > 0) || (!white_moved && mate < 0);
            favorable_to_mover
                && after
                    .mate_distance
                    .is_none_or(|later| later.signum() != mate.signum() || later.abs() > mate.abs())
        });
        if loss < 75 && !mate_lost {
            continue;
        }
        let fullmove = before_pos.fullmoves().get();
        let label = if white_moved {
            format!("{fullmove}.{}", game.sans[ply])
        } else {
            format!("{fullmove}...{}", game.sans[ply])
        };
        let loss_label = if mate_lost {
            ui::i18n::t("Mate lost").to_owned()
        } else {
            format!("−{:.2}", f64::from(loss) / 100.0)
        };
        let mut text = if mate_lost {
            format!(
                "Ply {} {} gives up a forced mate in {} for {}.",
                ply + 1,
                game.sans[ply],
                before.mate_distance.unwrap_or_default().abs(),
                if white_moved {
                    ui::i18n::t("White")
                } else {
                    ui::i18n::t("Black")
                },
            )
        } else {
            format!(
                "Ply {} {}: engine score lost {:.2} pawns for {}.",
                ply + 1,
                game.sans[ply],
                f64::from(loss) / 100.0,
                if white_moved {
                    ui::i18n::t("White")
                } else {
                    ui::i18n::t("Black")
                },
            )
        };
        let material_before = chess_core::material_balance(before_pos.board()).score;
        let material_after = chess_core::material_balance(after_pos.board()).score;
        let material_loss = if white_moved {
            material_before - material_after
        } else {
            material_after - material_before
        };
        if material_loss >= 1 {
            text.push_str(&format!(
                " Board material immediately drops by {material_loss} points."
            ));
        }
        let pv: Vec<String> = before
            .pv
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect();
        let sans = chess_core::pv_to_san(before_pos, &pv);
        if !sans.is_empty() {
            text.push_str(&format!(" Engine line: {}.", sans.join(" ")));
        }
        text.push_str(" Engine estimates can change with deeper searches.");
        // 4 is ??, 2 is ? and 6 is ?!.
        let nag = if mate_lost || loss >= 300 {
            4
        } else if loss >= 150 {
            2
        } else {
            6
        };
        findings.push(ReviewFinding {
            ply,
            label,
            loss: loss_label,
            text,
            nag,
            best: chess_core::format_pv((fullmove - 1) * 2 + u32::from(!white_moved), &sans),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_is_bounded_and_monotonic() {
        assert!((accuracy_from_loss(0.0) - 100.0).abs() < 0.01);
        assert!(accuracy_from_loss(5.0) > accuracy_from_loss(10.0));
        assert!(accuracy_from_loss(1000.0) >= 0.0);
        assert!((win_percent(0) - 50.0).abs() < 0.01);
        assert!(win_percent(500) > 80.0);
    }
}
