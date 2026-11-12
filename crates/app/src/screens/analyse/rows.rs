//! The rows of the moves table: engine lines, database moves and repertoire moves merged into one list per move.

use super::*;

/// One candidate move: an engine's line for it, or a move only databases, online
/// sources or the repertoire know. With several engines a move can have a row each.
#[derive(Clone, Debug, Default)]
pub struct MoveRow {
    pub san: String,
    /// The score of this row's engine line.
    pub score: Option<Score>,
    pub wdl: Option<Wdl>,
    pub pv: Vec<String>,
    pub depth: u32,
    /// The engine whose line this row is.
    pub engine_id: Option<EngineId>,
    pub reference: Option<MoveStat>,
    pub mine: Option<MoveStat>,
    /// The tablebase's verdict on this move, while Online is on.
    pub tb: Option<crate::online::TbRow>,
    /// chessdb.cn's score for this move, while Online is on.
    pub cloud: Option<crate::online::CloudMove>,
    pub rep: RepTag,
    /// How this line compares with the best one, for its arrow and label colour.
    pub quality: Option<Quality>,
}

/// Which table row is picked: the move, and the engine whose line it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowKey {
    pub san: String,
    pub engine_id: Option<EngineId>,
}

impl RowKey {
    pub fn of(row: &MoveRow) -> Self {
        Self {
            san: row.san.clone(),
            engine_id: row.engine_id,
        }
    }

    pub fn matches(&self, row: &MoveRow) -> bool {
        row.san == self.san && row.engine_id == self.engine_id
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepTag {
    Main,
    Side,
    Deviate,
    #[default]
    None,
}

/// Where rows come from and how to order them.
#[derive(Clone, Copy, Debug)]
pub struct RowOptions {
    pub white_to_move: bool,
    pub user_to_move: bool,
    pub sort: LineSort,
}

/// Merges candidate sources into table rows, sorted by engine eval for the side to
/// move (or by depth, deepest first); moves with no eval follow, by popularity, then
/// any remaining legal moves in `legal`, so every candidate is listed.
pub fn merge_rows(
    engine_lines: Vec<MoveRow>,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    rep_moves: &[(String, bool)],
    legal: &[String],
    options: RowOptions,
) -> Vec<MoveRow> {
    let RowOptions {
        white_to_move,
        user_to_move,
        sort,
    } = options;
    let mut rows = engine_lines;
    for stat in reference {
        for row in rows_for(&mut rows, &stat.san, true) {
            row.reference = Some(stat.clone());
        }
    }
    for stat in mine {
        for row in rows_for(&mut rows, &stat.san, true) {
            row.mine = Some(stat.clone());
        }
    }
    for (san, main) in rep_moves {
        for row in rows_for(&mut rows, san, true) {
            row.rep = if *main { RepTag::Main } else { RepTag::Side };
        }
    }
    for san in legal {
        // Adding the row is the point; there is nothing to set on it.
        let _ = rows_for(&mut rows, san, true);
    }
    // The user played something the repertoire does not cover, when it has an answer.
    if user_to_move && !rep_moves.is_empty() {
        for row in &mut rows {
            if row.rep == RepTag::None && row.mine.is_some() {
                row.rep = RepTag::Deviate;
            }
        }
    }
    let popularity = |row: &MoveRow| {
        row.reference.as_ref().map_or(0, |stat| stat.games)
            + row.mine.as_ref().map_or(0, |stat| stat.games)
    };
    // Without an engine eval, online verdicts order what popularity leaves tied: the
    // tablebase's result first, then the cloud's score, both for the side to move.
    let tb_rank = |row: &MoveRow| row.tb.as_ref().map(|tb| online::outcome_rank(tb.outcome));
    let cloud_score = |row: &MoveRow| row.cloud.as_ref().map(|cloud| cloud.score);
    let key = |row: &MoveRow| {
        row.score
            .map(|score| score.sort_key() * if white_to_move { 1 } else { -1 })
    };
    rows.sort_by(|a, b| {
        let by_eval = match (key(a), key(b)) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => popularity(b)
                .cmp(&popularity(a))
                .then(tb_rank(b).cmp(&tb_rank(a)))
                .then(cloud_score(b).cmp(&cloud_score(a))),
        };
        match sort {
            LineSort::Eval => by_eval,
            LineSort::Depth => b.depth.cmp(&a.depth).then(by_eval),
        }
    });
    // Grade each line against the best one, from the side to move's view.
    let relative = |score: Score| {
        if white_to_move { score } else { score.negate() }
    };
    let best = rows
        .iter()
        .filter_map(|row| row.score.map(relative))
        .max_by_key(|score| score.sort_key());
    if let Some(best) = best {
        for row in &mut rows {
            row.quality = row
                .score
                .map(|score| engine::classify_move(best, relative(score)));
        }
    }
    rows
}

/// Every row for `san` (one per engine that found it), ignoring check marks. When
/// there is none and `add` is set, a new row for the move is added and returned.
pub(crate) fn rows_for<'a>(
    rows: &'a mut Vec<MoveRow>,
    san: &str,
    add: bool,
) -> impl Iterator<Item = &'a mut MoveRow> {
    fn clean(san: &str) -> &str {
        san.trim_end_matches(['+', '#'])
    }
    if add && !rows.iter().any(|row| clean(&row.san) == clean(san)) {
        rows.push(MoveRow {
            san: san.to_owned(),
            ..Default::default()
        });
    }
    rows.iter_mut()
        .filter(move |row| clean(&row.san) == clean(san))
}

/// The colour for a move's quality.
pub fn quality_color(quality: Quality) -> u32 {
    match quality {
        Quality::Best => T.q_best,
        Quality::Good => T.q_good,
        Quality::Inaccuracy => T.q_inaccuracy,
        Quality::Mistake => T.q_mistake,
        Quality::Blunder => T.q_blunder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(san: &str, games: u64) -> MoveStat {
        MoveStat {
            san: san.to_owned(),
            games,
            white_wins: games,
            draws: 0,
            black_wins: 0,
        }
    }

    fn opts(white_to_move: bool, user_to_move: bool) -> RowOptions {
        RowOptions {
            white_to_move,
            user_to_move,
            sort: LineSort::Eval,
        }
    }

    fn line(san: &str, cp: i32) -> MoveRow {
        MoveRow {
            san: san.to_owned(),
            score: Some(Score::Cp(cp)),
            ..Default::default()
        }
    }

    #[test]
    fn rows_without_an_eval_follow_the_tablebase_then_the_cloud() {
        use crate::online::{CloudMove, Outcome, TbRow};
        let tb = |san: &str, outcome| MoveRow {
            san: san.to_owned(),
            tb: Some(TbRow {
                san: san.to_owned(),
                outcome,
                dtz: None,
                dtm: None,
                is_mate: false,
                draw_reason: None,
            }),
            ..Default::default()
        };
        let cloud = |san: &str, score| MoveRow {
            san: san.to_owned(),
            cloud: Some(CloudMove {
                san: san.to_owned(),
                score,
                rank: 0,
                note: String::new(),
                winrate: None,
            }),
            ..Default::default()
        };
        let rows = merge_rows(
            vec![
                cloud("a3", -40),
                tb("Kd2", Outcome::Draw),
                cloud("e4", 35),
                tb("Rd8", Outcome::Win),
                line("Nf3", 20),
            ],
            vec![],
            vec![],
            &[],
            &[],
            opts(true, false),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["Nf3", "Rd8", "Kd2", "e4", "a3"]);
    }

    #[test]
    fn rows_are_the_union_sorted_by_eval_then_popularity() {
        let rows = merge_rows(
            vec![line("h4", 31), line("f4", 42)],
            vec![stat("f4", 412), stat("Kb1", 131), stat("Ng3", 47)],
            vec![stat("h4", 9)],
            &[("f4".to_owned(), true), ("Ng3".to_owned(), false)],
            &[],
            opts(true, true),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["f4", "h4", "Kb1", "Ng3"]);
        assert_eq!(rows[0].rep, RepTag::Main);
        assert_eq!(rows[1].rep, RepTag::Deviate);
        assert_eq!(rows[2].rep, RepTag::None);
        assert_eq!(rows[3].rep, RepTag::Side);
        assert_eq!(rows[0].reference.as_ref().unwrap().games, 412);
    }

    #[test]
    fn each_engine_line_keeps_its_own_row_and_shares_the_move_data() {
        let engine_line = |san: &str, cp: i32, id: EngineId| MoveRow {
            engine_id: Some(id),
            ..line(san, cp)
        };
        let rows = merge_rows(
            vec![
                engine_line("d4", 24, 1),
                engine_line("d4", 31, 2),
                engine_line("e4", 19, 2),
            ],
            vec![stat("d4", 95), stat("c4", 7)],
            vec![],
            &[],
            &[],
            opts(true, true),
        );
        let order: Vec<(&str, Option<EngineId>)> = rows
            .iter()
            .map(|row| (row.san.as_str(), row.engine_id))
            .collect();
        assert_eq!(
            order,
            vec![
                ("d4", Some(2)),
                ("d4", Some(1)),
                ("e4", Some(2)),
                ("c4", None),
            ]
        );
        assert_eq!(rows[0].reference.as_ref().unwrap().games, 95);
        assert_eq!(rows[1].reference.as_ref().unwrap().games, 95);
    }

    #[test]
    fn black_prefers_lower_white_relative_scores() {
        let rows = merge_rows(
            vec![line("a", 50), line("b", -50)],
            vec![],
            vec![],
            &[],
            &[],
            opts(false, false),
        );
        assert_eq!(rows[0].san, "b");
    }

    #[test]
    fn check_suffixes_merge_with_database_moves() {
        let rows = merge_rows(
            vec![line("Qh5+", 10)],
            vec![stat("Qh5", 3)],
            vec![],
            &[],
            &[],
            opts(true, true),
        );
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn no_repertoire_means_no_deviation_tag() {
        let rows = merge_rows(
            vec![],
            vec![],
            vec![stat("e4", 3)],
            &[],
            &[],
            opts(true, true),
        );
        assert_eq!(rows[0].rep, RepTag::None);
        // Nor on the opponent's turn, where the moves are not the user's choices.
        let rows = merge_rows(
            vec![],
            vec![],
            vec![stat("e5", 3)],
            &[("c5".to_owned(), true)],
            &[],
            opts(false, false),
        );
        assert_eq!(
            rows.iter().find(|row| row.san == "e5").unwrap().rep,
            RepTag::None
        );
    }

    #[test]
    fn depth_sort_puts_deeper_lines_first() {
        let mut shallow = line("a", 90);
        shallow.depth = 10;
        let mut deep = line("b", 20);
        deep.depth = 30;
        let rows = merge_rows(
            vec![shallow, deep],
            vec![],
            vec![],
            &[],
            &[],
            RowOptions {
                sort: LineSort::Depth,
                ..opts(true, true)
            },
        );
        assert_eq!(rows[0].san, "b");
    }

    #[test]
    fn all_legal_moves_are_listed_once_after_analysed_ones() {
        let rows = merge_rows(
            vec![line("e4", 30)],
            vec![],
            vec![],
            &[],
            &["e4".to_owned(), "d4".to_owned(), "Nf3".to_owned()],
            opts(true, true),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["e4", "d4", "Nf3"]);
        assert!(rows[1].score.is_none());
    }

    #[test]
    fn lines_are_graded_against_the_best_for_the_side_to_move() {
        let rows = merge_rows(
            vec![line("a", -40), line("b", 300), line("c", 0)],
            vec![],
            vec![],
            &[],
            &[],
            opts(false, false),
        );
        // Black to move: -0.40 is best for Black, +3.00 throws the game away.
        assert_eq!(rows[0].san, "a");
        assert_eq!(rows[0].quality, Some(Quality::Best));
        assert_eq!(rows[2].san, "b");
        assert_eq!(rows[2].quality, Some(Quality::Blunder));
        assert!(rows[1].quality > Some(Quality::Best));
    }
}
