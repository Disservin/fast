//! Writing a played game as PGN: headers, player names and each engine move's score and time.

use super::*;

/// Writes move time, the optional game clock and a White-relative score for PGN export.
pub(super) fn match_move_comment(
    elapsed_ms: u64,
    clock_ms: Option<u64>,
    score: Option<engine::Score>,
    depth: u32,
) -> String {
    let time = |ms: u64| {
        format!(
            "{}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1_000 % 60,
            ms % 1_000
        )
    };
    let mut commands = vec![format!("[%emt {}]", time(elapsed_ms))];
    if let Some(ms) = clock_ms {
        commands.push(format!("[%clk {}]", time(ms)));
    }
    if let Some(score) = score {
        let value = match score {
            engine::Score::Cp(cp) => format!("{:.2}", f64::from(cp) / 100.0),
            engine::Score::Mate(mate) => format!("#{mate}"),
        };
        commands.push(if depth > 0 {
            format!("[%eval {value},{depth}]")
        } else {
            format!("[%eval {value}]")
        });
    }
    commands.join(" ")
}

/// Names an engine in game records using its own identification when available.
pub(super) fn engine_game_name(config: &EngineConfig) -> String {
    if config.info.name.trim().is_empty() {
        config.name.clone()
    } else {
        config.info.name.clone()
    }
}

/// Records the participants and UTC start date before moves or recovery saves are made.
pub(super) fn set_game_headers(
    tree: &mut GameTree,
    white: &str,
    black: &str,
    engines: bool,
    date: chrono::NaiveDate,
) {
    tree.set_header(
        "Event",
        if engines {
            "Fast engine match"
        } else {
            "Fast casual game"
        },
    );
    tree.set_header("Site", "Fast");
    tree.set_header("Date", &date.format("%Y.%m.%d").to_string());
    tree.set_header("Round", "-");
    tree.set_header("White", white);
    tree.set_header("Black", black);
    tree.set_header("Result", "*");
}

/// Formats a move's saved evaluation and thinking time for the engine match scoresheet.
pub(super) fn match_move_values(comment: &str) -> (String, String) {
    let note = chess_core::annotation::parse_annotation(comment);
    let eval = if let Some(mate) = note.mate_distance {
        format!("#{mate}")
    } else {
        note.eval_cp.map_or_else(
            || "—".to_owned(),
            |cp| format!("{:+.2}", f64::from(cp) / 100.0),
        )
    };
    let time = note.elapsed.map_or_else(
        || "—".to_owned(),
        |seconds| {
            if seconds >= 60.0 {
                format_clock((seconds * 1000.0) as u64)
            } else {
                format!("{seconds:.1}s")
            }
        },
    );
    (eval, time)
}

pub(super) fn opponent_names(engines: &[EngineConfig]) -> Vec<SharedString> {
    if engines.is_empty() {
        return vec![ui::i18n::t("No engine configured").into()];
    }
    engines
        .iter()
        .map(|engine| {
            let limited = engine.info.options.iter().any(|option| {
                option.name == "UCI_Elo" && matches!(option.kind, OptionKind::Spin { .. })
            }) || engine.name.eq_ignore_ascii_case("stockfish");
            let version = if engine.version.is_empty() {
                String::new()
            } else {
                format!(" {}", engine.version)
            };
            let suffix = if limited { ", limited strength" } else { "" };
            format!("{}{version}{suffix}", engine.name).into()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Move rows preserve White-relative scores, mate distances and missing values.
    #[test]
    fn match_move_values_show_scores_and_time() {
        assert_eq!(match_move_values(""), ("—".into(), "—".into()));
        assert_eq!(
            match_move_values("[%eval -0.42,18] [%emt 0:00:01.250]"),
            ("-0.42".into(), "1.2s".into())
        );
        assert_eq!(
            match_move_values("[%eval #-3] [%emt 0:01:05.000]"),
            ("#-3".into(), "01:05".into())
        );
    }

    /// Game records replace source metadata and retain their participants through PGN export.
    #[test]
    fn played_game_headers_survive_export() {
        let mut tree = GameTree::new();
        tree.set_header("Event", "Opening book source");
        tree.set_header("Date", "1999.01.01");
        tree.set_header("Result", "1-0");
        set_game_headers(
            &mut tree,
            "Stockfish 18",
            "Berserk 13",
            true,
            chrono::NaiveDate::from_ymd_opt(2024, 10, 4).unwrap(),
        );
        let exported = chess_core::write_pgn(&tree);
        let restored = chess_core::parse_pgn(&exported).unwrap().remove(0);
        for (key, expected) in [
            ("Event", "Fast engine match"),
            ("Site", "Fast"),
            ("Date", "2024.10.04"),
            ("Round", "-"),
            ("White", "Stockfish 18"),
            ("Black", "Berserk 13"),
            ("Result", "*"),
        ] {
            assert_eq!(restored.header(key), Some(expected));
        }
    }

    /// Human games keep the player's selected colour and use the casual event name.
    #[test]
    fn played_game_human_participants() {
        let mut tree = GameTree::new();
        set_game_headers(
            &mut tree,
            "Stockfish 18",
            "Player",
            false,
            chrono::NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(),
        );
        assert_eq!(tree.header("Date"), Some("2024.02.29"));
        assert_eq!(tree.header("Event"), Some("Fast casual game"));
        assert_eq!(tree.header("Black"), Some("Player"));
    }

    /// Exported match annotations retain precise times and White-relative scores.
    #[test]
    fn match_annotations_survive_pgn_export() {
        let white = match_move_comment(1_234, Some(60_766), Some(engine::Score::Cp(35)), 18);
        let black = match_move_comment(2_345, Some(59_655), Some(engine::Score::Cp(-42)), 20);
        let tree = chess_core::parse_pgn(&format!("1. e4 {{{white}}} e5 {{{black}}} *"))
            .unwrap()
            .remove(0);
        let exported = chess_core::write_pgn(&tree);
        let restored = chess_core::parse_pgn(&exported).unwrap().remove(0);
        let moves = restored.line_from(GameTree::ROOT);
        let note = chess_core::annotation::parse_annotation(&restored.node(moves[0]).comment);
        assert_eq!(note.elapsed, Some(1.234));
        assert_eq!(note.clock, Some(60.766));
        assert_eq!(note.eval_cp, Some(35));
        assert_eq!(note.depth, Some(18));
        let note = chess_core::annotation::parse_annotation(&restored.node(moves[1]).comment);
        assert_eq!(note.eval_cp, Some(-42));
        assert_eq!(note.depth, Some(20));
        assert_eq!(note.elapsed, Some(2.345));
    }

    /// Per-move limits have elapsed time without a fictitious remaining clock or score.
    #[test]
    fn match_annotations_handle_limits_missing_scores_and_mates() {
        let comment = match_move_comment(10, None, None, 0);
        let note = chess_core::annotation::parse_annotation(&comment);
        assert_eq!(note.elapsed, Some(0.01));
        assert_eq!(note.clock, None);
        assert_eq!(note.eval_cp, None);
        let comment = match_move_comment(3_661_001, None, Some(engine::Score::Mate(-3)), 25);
        assert!(comment.contains("[%emt 1:01:01.001]"));
        assert!(comment.contains("[%eval #-3,25]"));
    }
}
