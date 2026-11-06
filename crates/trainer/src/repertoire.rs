//! A repertoire: a move tree for one side, with a scheduling card per user move.

use std::collections::HashMap;

use chess_core::{
    Chess, Color, Position,
    shakmaty::san::{San, SanPlus},
};
use serde::{Deserialize, Serialize};

use crate::{Card, CardState, Grade};

/// Displayed status of a line or move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepStatus {
    Known,
    Learning,
    Due,
    New,
}

/// A move in the repertoire tree. Stored as SAN so files stay human-readable.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepNode {
    pub san: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<RepNode>,
    /// Hand-curated tree row. When any node has one, only such nodes are listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<CuratedRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CuratedRow {
    pub depth: usize,
    pub label: String,
}

/// One row of the repertoire tree as displayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub status: RepStatus,
    /// SAN path to the row's first move.
    pub path: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repertoire {
    pub name: String,
    pub white: bool,
    pub roots: Vec<RepNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub known: usize,
    pub learning: usize,
    pub new: usize,
}

impl Coverage {
    pub fn total(&self) -> usize {
        self.known + self.learning + self.new
    }

    /// Whole-number percentages of known, learning, new.
    pub fn percents(&self) -> (u32, u32, u32) {
        let total = self.total().max(1) as f32;
        let pct = |count: usize| (count as f32 * 100.0 / total).round() as u32;
        (pct(self.known), pct(self.learning), pct(self.new))
    }
}

/// A due or new user move to quiz: the moves leading to it and the expected answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drill {
    pub path: Vec<String>,
    pub answer: String,
    pub note: String,
}

impl Repertoire {
    pub fn new(name: impl Into<String>, white: bool) -> Self {
        Self {
            name: name.into(),
            white,
            roots: Vec::new(),
        }
    }

    pub fn user_color(&self) -> Color {
        if self.white {
            Color::White
        } else {
            Color::Black
        }
    }

    /// Whether the node at this depth (0 = White's first move) is the user's move.
    /// Assumes games start from the initial position, as repertoire lines do.
    fn is_user_move(&self, depth: usize) -> bool {
        depth.is_multiple_of(2) == self.white
    }

    /// Inserts a SAN line, reusing existing moves. Validates legality.
    pub fn add_line(&mut self, sans: &[&str]) -> Result<(), String> {
        let mut pos = Chess::default();
        for san in sans {
            let parsed: San = san.parse().map_err(|err| format!("{san}: {err}"))?;
            let mv = parsed
                .to_move(&pos)
                .map_err(|err| format!("{san}: {err}"))?;
            pos = pos.play(mv).map_err(|err| format!("{san}: {err}"))?;
        }
        let white = self.white;
        let mut level = &mut self.roots;
        for (depth, san) in sans.iter().enumerate() {
            let canonical = san.trim_end_matches(['+', '#', '!', '?']);
            let index = match level.iter().position(|node| node.san == canonical) {
                Some(index) => index,
                None => {
                    level.push(RepNode {
                        san: canonical.to_owned(),
                        card: ((depth % 2 == 0) == white).then(Card::default),
                        ..Default::default()
                    });
                    level.len() - 1
                }
            };
            level = &mut level[index].children;
        }
        Ok(())
    }

    /// Finds a node by its SAN path from the start position.
    pub fn node(&self, path: &[impl AsRef<str>]) -> Option<&RepNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self
            .roots
            .iter()
            .find(|node| node.san == clean(first.as_ref()))?;
        for san in rest {
            node = node
                .children
                .iter()
                .find(|child| child.san == clean(san.as_ref()))?;
        }
        Some(node)
    }

    fn node_mut(&mut self, path: &[impl AsRef<str>]) -> Option<&mut RepNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self
            .roots
            .iter_mut()
            .find(|node| node.san == clean(first.as_ref()))?;
        for san in rest {
            node = node
                .children
                .iter_mut()
                .find(|child| child.san == clean(san.as_ref()))?;
        }
        Some(node)
    }

    /// Repertoire moves available after the given path: `None` once out of book.
    pub fn moves_after(&self, path: &[impl AsRef<str>]) -> Option<&[RepNode]> {
        if path.is_empty() {
            return Some(&self.roots);
        }
        self.node(path).map(|node| node.children.as_slice())
    }

    pub fn set_card(&mut self, path: &[impl AsRef<str>], card: Card) {
        if let Some(node) = self.node_mut(path) {
            node.card = Some(card);
        }
    }

    pub fn grade(&mut self, path: &[impl AsRef<str>], grade: Grade, today: i64) {
        if let Some(node) = self.node_mut(path) {
            let card = node.card.unwrap_or_default();
            node.card = Some(card.answered(grade, today));
        }
    }

    pub fn card_status(card: Option<&Card>, today: i64) -> RepStatus {
        match card {
            None => RepStatus::Known,
            Some(card) if card.is_due(today) => RepStatus::Due,
            Some(card) => match card.state {
                CardState::New => RepStatus::New,
                CardState::Learning => RepStatus::Learning,
                // Short intervals are still being learned.
                CardState::Review if card.interval_days < 7 => RepStatus::Learning,
                CardState::Review => RepStatus::Known,
            },
        }
    }

    /// Status of a subtree's own line: the most urgent of its unbranched moves.
    pub fn line_status(node: &RepNode, today: i64) -> RepStatus {
        let mut status = Self::card_status(node.card.as_ref(), today);
        let mut current = node;
        while current.children.len() == 1 {
            current = &current.children[0];
            let next = Self::card_status(current.card.as_ref(), today);
            status = worst(status, next);
        }
        status
    }

    pub fn coverage(&self, today: i64) -> Coverage {
        fn walk(nodes: &[RepNode], today: i64, coverage: &mut Coverage) {
            for node in nodes {
                if let Some(card) = &node.card {
                    match Repertoire::card_status(Some(card), today) {
                        RepStatus::Known => coverage.known += 1,
                        RepStatus::Learning | RepStatus::Due => coverage.learning += 1,
                        RepStatus::New => coverage.new += 1,
                    }
                }
                walk(&node.children, today, coverage);
            }
        }
        let mut coverage = Coverage::default();
        walk(&self.roots, today, &mut coverage);
        coverage
    }

    /// The tree as rows: unbranched chains collapse into one row, branches indent.
    pub fn tree_rows(&self, today: i64) -> Vec<TreeRow> {
        fn has_curated(nodes: &[RepNode]) -> bool {
            nodes
                .iter()
                .any(|node| node.row.is_some() || has_curated(&node.children))
        }
        /// A node's status, looking one move ahead when it is an opponent move.
        fn own_status(node: &RepNode, today: i64) -> RepStatus {
            let card = node
                .card
                .as_ref()
                .or_else(|| node.children.first().and_then(|child| child.card.as_ref()));
            match card {
                Some(_) => Repertoire::card_status(card, today),
                None => RepStatus::Known,
            }
        }
        fn curated(nodes: &[RepNode], path: &mut Vec<String>, today: i64, rows: &mut Vec<TreeRow>) {
            for node in nodes {
                if let Some(row) = &node.row {
                    let mut row_path = path.clone();
                    row_path.push(node.san.clone());
                    rows.push(TreeRow {
                        depth: row.depth,
                        label: row.label.clone(),
                        status: own_status(node, today),
                        path: row_path,
                    });
                }
                path.push(node.san.clone());
                curated(&node.children, path, today, rows);
                path.pop();
            }
        }
        fn automatic(
            nodes: &[RepNode],
            depth: usize,
            path: &mut Vec<String>,
            today: i64,
            rows: &mut Vec<TreeRow>,
        ) {
            for node in nodes {
                let start_len = path.len();
                let row_path = {
                    let mut row_path = path.clone();
                    row_path.push(node.san.clone());
                    row_path
                };
                let mut chain = vec![node];
                let mut last = node;
                while last.children.len() == 1 {
                    last = &last.children[0];
                    chain.push(last);
                }
                let shown: Vec<String> = chain
                    .iter()
                    .take(MAX_ROW_PLIES)
                    .map(|n| n.san.clone())
                    .collect();
                let mut label = chess_core::format_pv(start_len as u32, &shown);
                if chain.len() > MAX_ROW_PLIES {
                    label.push_str(" …");
                }
                if !node.label.is_empty() {
                    label = format!("{label} {}", node.label);
                }
                rows.push(TreeRow {
                    depth,
                    label,
                    status: Repertoire::line_status(node, today),
                    path: row_path,
                });
                path.extend(chain.iter().map(|n| n.san.clone()));
                automatic(&last.children, depth + 1, path, today, rows);
                path.truncate(start_len);
            }
        }
        let mut rows = Vec::new();
        if has_curated(&self.roots) {
            curated(&self.roots, &mut Vec::new(), today, &mut rows);
        } else {
            automatic(&self.roots, 0, &mut Vec::new(), today, &mut rows);
        }
        rows
    }

    /// Number of positions (moves) in the tree.
    pub fn positions(&self) -> usize {
        fn count(nodes: &[RepNode]) -> usize {
            nodes.iter().map(|node| 1 + count(&node.children)).sum()
        }
        count(&self.roots)
    }

    /// All user moves that are due, oldest due day first, then new cards in tree order.
    pub fn drills(&self, today: i64) -> Vec<Drill> {
        fn walk(
            repertoire: &Repertoire,
            nodes: &[RepNode],
            path: &mut Vec<String>,
            today: i64,
            due: &mut Vec<(i64, Drill)>,
            new: &mut Vec<Drill>,
        ) {
            for node in nodes {
                if repertoire.is_user_move(path.len())
                    && let Some(card) = &node.card
                {
                    let drill = Drill {
                        path: path.clone(),
                        answer: node.san.clone(),
                        note: node.note.clone(),
                    };
                    if card.is_due(today) {
                        due.push((card.due_day, drill));
                    } else if card.state == CardState::New {
                        new.push(drill);
                    }
                }
                path.push(node.san.clone());
                walk(repertoire, &node.children, path, today, due, new);
                path.pop();
            }
        }
        let mut due = Vec::new();
        let mut new = Vec::new();
        walk(
            self,
            &self.roots,
            &mut Vec::new(),
            today,
            &mut due,
            &mut new,
        );
        due.sort_by_key(|(day, _)| *day);
        due.into_iter().map(|(_, drill)| drill).chain(new).collect()
    }

    /// Positions where the user is to move and the repertoire has no answer.
    pub fn gaps(&self) -> Vec<Vec<String>> {
        fn walk(
            repertoire: &Repertoire,
            nodes: &[RepNode],
            path: &mut Vec<String>,
            gaps: &mut Vec<Vec<String>>,
        ) {
            for node in nodes {
                path.push(node.san.clone());
                if node.children.is_empty() && repertoire.is_user_move(path.len()) {
                    gaps.push(path.clone());
                }
                walk(repertoire, &node.children, path, gaps);
                path.pop();
            }
        }
        let mut gaps = Vec::new();
        walk(self, &self.roots, &mut Vec::new(), &mut gaps);
        gaps
    }

    /// Hash → repertoire moves, for fast lookups by position (handles transpositions).
    pub fn position_index(&self) -> HashMap<u64, Vec<(String, bool)>> {
        fn walk(nodes: &[RepNode], pos: &Chess, index: &mut HashMap<u64, Vec<(String, bool)>>) {
            let hash = chess_core::position_hash(pos);
            for (rank, node) in nodes.iter().enumerate() {
                index
                    .entry(hash)
                    .or_default()
                    .push((node.san.clone(), rank == 0));
                let Ok(san) = node.san.parse::<San>() else {
                    continue;
                };
                let Ok(mv) = san.to_move(pos) else { continue };
                let mut next = pos.clone();
                let _ = SanPlus::from_move_and_play_unchecked(&mut next, mv);
                walk(&node.children, &next, index);
            }
        }
        let mut index = HashMap::new();
        walk(&self.roots, &Chess::default(), &mut index);
        index
    }
}

const MAX_ROW_PLIES: usize = 6;

fn clean(san: &str) -> &str {
    san.trim_end_matches(['+', '#', '!', '?'])
}

fn worst(a: RepStatus, b: RepStatus) -> RepStatus {
    let rank = |status| match status {
        RepStatus::Known => 0,
        RepStatus::Learning => 1,
        RepStatus::New => 2,
        RepStatus::Due => 3,
    };
    if rank(b) > rank(a) { b } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Repertoire {
        let mut rep = Repertoire::new("White vs Sicilian", true);
        rep.add_line(&["e4", "c5", "Nf3", "d6", "d4"]).unwrap();
        rep.add_line(&["e4", "c5", "Nf3", "Nc6", "Bb5"]).unwrap();
        rep.add_line(&["e4", "e5"]).unwrap();
        rep
    }

    #[test]
    fn lines_merge_and_only_user_moves_get_cards() {
        let rep = sample();
        assert_eq!(rep.roots.len(), 1);
        assert_eq!(rep.positions(), 8);
        assert!(rep.node(&["e4"]).unwrap().card.is_some());
        assert!(rep.node(&["e4", "c5"]).unwrap().card.is_none());
        assert_eq!(rep.moves_after(&["e4", "c5", "Nf3"]).unwrap().len(), 2);
        assert!(rep.moves_after(&["d4"]).is_none());
    }

    #[test]
    fn illegal_lines_are_rejected_without_partial_insert() {
        let mut rep = Repertoire::new("x", true);
        assert!(rep.add_line(&["e4", "e4"]).is_err());
        assert!(rep.roots.is_empty());
    }

    #[test]
    fn drills_put_due_before_new_and_sort_by_due_day() {
        let mut rep = sample();
        let mut later = Card::default().answered(Grade::Good, 0);
        later.due_day = 9;
        let mut earlier = Card::default().answered(Grade::Good, 0);
        earlier.due_day = 5;
        rep.set_card(&["e4", "c5", "Nf3", "d6", "d4"], later);
        rep.set_card(&["e4", "c5", "Nf3", "Nc6", "Bb5"], earlier);
        let drills = rep.drills(10);
        assert_eq!(drills[0].answer, "Bb5");
        assert_eq!(drills[1].answer, "d4");
        assert_eq!(drills[2].answer, "e4");
        assert_eq!(drills[2].path.len(), 0);
        assert_eq!(drills[3].answer, "Nf3");
    }

    #[test]
    fn grading_updates_status_and_coverage() {
        let mut rep = sample();
        assert_eq!(
            rep.coverage(0),
            Coverage {
                known: 0,
                learning: 0,
                new: 4
            }
        );
        rep.grade(&["e4"], Grade::Easy, 0);
        rep.grade(&["e4", "c5", "Nf3"], Grade::Hard, 0);
        assert_eq!(
            rep.coverage(0),
            Coverage {
                known: 1,
                learning: 1,
                new: 2
            }
        );
        assert_eq!(
            Repertoire::card_status(rep.node(&["e4"]).unwrap().card.as_ref(), 11),
            RepStatus::Due
        );
    }

    #[test]
    fn gaps_list_unanswered_user_turns() {
        let rep = sample();
        assert_eq!(rep.gaps(), vec![vec!["e4".to_owned(), "e5".to_owned()]]);
    }

    #[test]
    fn position_index_finds_moves_by_hash() {
        let rep = sample();
        let index = rep.position_index();
        let start = chess_core::position_hash(&Chess::default());
        assert_eq!(index[&start], vec![("e4".to_owned(), true)]);
    }

    #[test]
    fn automatic_rows_collapse_chains_and_indent_branches() {
        let rep = sample();
        let rows = rep.tree_rows(0);
        let labels: Vec<(&str, usize)> = rows
            .iter()
            .map(|row| (row.label.as_str(), row.depth))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("1.e4", 0),
                ("1…c5 2.Nf3", 1),
                ("2…d6 3.d4", 2),
                ("2…Nc6 3.Bb5", 2),
                ("1…e5", 1),
            ]
        );
        assert_eq!(rows[0].status, RepStatus::New);
    }

    #[test]
    fn curated_rows_replace_the_automatic_listing() {
        let mut rep = sample();
        rep.roots[0].row = Some(CuratedRow {
            depth: 0,
            label: "King's pawn".to_owned(),
        });
        let rows = rep.tree_rows(0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "King's pawn");
    }

    #[test]
    fn serde_round_trip() {
        let rep = sample();
        let text = serde_json::to_string(&rep).unwrap();
        let back: Repertoire = serde_json::from_str(&text).unwrap();
        assert_eq!(back.positions(), rep.positions());
    }
}
