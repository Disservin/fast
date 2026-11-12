//! A repertoire: a move tree for one side, with a scheduling card per user move.

use std::collections::HashMap;

use chess_core::{Chess, Position, shakmaty::san::San};
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
    #[serde(default)]
    pub history: Vec<crate::ReviewEvent>,
    #[serde(default)]
    pub session: Option<crate::StudySession>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub legacy_cards: std::collections::BTreeMap<String, Card>,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            history: Vec::new(),
            session: None,
            legacy_cards: Default::default(),
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
        self.migrate_cards();
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
    pub fn position_key(path: &[impl AsRef<str>]) -> Option<String> {
        let mut pos = Chess::default();
        for text in path {
            let san: San = text.as_ref().parse().ok()?;
            let mv = san.to_move(&pos).ok()?;
            pos = pos.play(mv).ok()?;
        }
        Some(identity(&pos))
    }

    fn entries(&self) -> Vec<(String, Vec<String>, &RepNode)> {
        fn walk<'a>(
            nodes: &'a [RepNode],
            pos: &Chess,
            path: &mut Vec<String>,
            out: &mut Vec<(String, Vec<String>, &'a RepNode)>,
        ) {
            for node in nodes {
                let Ok(san) = node.san.parse::<San>() else {
                    continue;
                };
                let Ok(mv) = san.to_move(pos) else { continue };
                path.push(node.san.clone());
                out.push((identity(pos), path.clone(), node));
                if let Ok(next) = pos.clone().play(mv) {
                    walk(&node.children, &next, path, out);
                }
                path.pop();
            }
        }
        let mut out = Vec::new();
        walk(&self.roots, &Chess::default(), &mut Vec::new(), &mut out);
        out
    }

    /// Full position identity is compared; clocks do not split opening knowledge.
    pub fn moves_after(&self, path: &[impl AsRef<str>]) -> Option<Vec<&RepNode>> {
        let key = Self::position_key(path)?;
        let mut moves = Vec::new();
        for (position, _, node) in self.entries() {
            if position == key && !moves.iter().any(|n: &&RepNode| n.san == node.san) {
                moves.push(node);
            }
        }
        if !moves.is_empty() || self.node(path).is_some() || path.is_empty() {
            Some(moves)
        } else {
            None
        }
    }

    /// Merge legacy path cards conservatively: keep the most reviewed card, then
    /// propagate it to all occurrences without resetting its schedule or lapses.
    pub fn migrate_cards(&mut self) {
        let legacy: Vec<_> = self
            .entries()
            .into_iter()
            .filter_map(|(_, path, node)| {
                node.card
                    .filter(|card| card.state != CardState::New)
                    .map(|card| (path.join(" "), card))
            })
            .collect();
        for (path, card) in legacy {
            self.legacy_cards.entry(path).or_insert(card);
        }
        let mut shared: HashMap<(String, String), Card> = HashMap::new();
        for (key, _, node) in self.entries() {
            if let Some(card) = node.card {
                shared
                    .entry((key, node.san.clone()))
                    .and_modify(|old| {
                        if (
                            card.state != CardState::New,
                            card.lapses,
                            card.due_day,
                            card.interval_days,
                        ) > (
                            old.state != CardState::New,
                            old.lapses,
                            old.due_day,
                            old.interval_days,
                        ) {
                            *old = card;
                        }
                    })
                    .or_insert(card);
            }
        }
        let updates: Vec<_> = self
            .entries()
            .into_iter()
            .filter_map(|(key, path, node)| {
                shared
                    .get(&(key, node.san.clone()))
                    .copied()
                    .map(|card| (path, card))
            })
            .collect();
        for (path, card) in updates {
            if let Some(node) = self.node_mut(&path) {
                node.card = Some(card);
            }
        }
    }

    pub fn set_card(&mut self, path: &[impl AsRef<str>], card: Card) {
        let Some((answer, before)) = path.split_last() else {
            return;
        };
        let Some(key) = Self::position_key(before) else {
            return;
        };
        let paths: Vec<_> = self
            .entries()
            .into_iter()
            .filter(|(position, _, node)| *position == key && node.san == clean(answer.as_ref()))
            .map(|(_, path, _)| path)
            .collect();
        for path in paths {
            if let Some(node) = self.node_mut(&path) {
                node.card = Some(card);
            }
        }
    }

    pub fn card_for(&self, path: &[impl AsRef<str>]) -> Option<Card> {
        let (answer, before) = path.split_last()?;
        self.moves_after(before)?
            .into_iter()
            .find(|node| node.san == clean(answer.as_ref()))?
            .card
    }

    pub fn grade(&mut self, path: &[impl AsRef<str>], grade: Grade, today: i64) {
        self.migrate_cards();
        if let Some(card) = self.card_for(path) {
            self.set_card(path, card.answered(grade, today));
        }
    }

    pub fn submit_review(
        &mut self,
        id: u64,
        path: &[String],
        requested: Grade,
        attempt: crate::Attempt,
        day: i64,
    ) -> bool {
        if self.history.iter().any(|event| event.id == id) {
            return false;
        }
        let Some((answer, before_path)) = path.split_last() else {
            return false;
        };
        let Some(position) = Self::position_key(before_path) else {
            return false;
        };
        let Some(before) = self.moves_after(before_path).and_then(|moves| {
            moves
                .into_iter()
                .find(|node| node.san == *answer)
                .and_then(|node| node.card)
        }) else {
            return false;
        };
        let applied = attempt.scheduling_grade(requested);
        let after = before.answered(applied, day);
        self.set_card(path, after);
        self.history.push(crate::ReviewEvent {
            id,
            day,
            position,
            answer: answer.clone(),
            attempt,
            requested,
            applied,
            before,
            after,
        });
        true
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
        let mut coverage = Coverage::default();
        let mut seen = std::collections::HashSet::new();
        for (key, _, node) in self.entries() {
            if let Some(card) = &node.card
                && seen.insert((key, node.san.clone()))
            {
                match Self::card_status(Some(card), today) {
                    RepStatus::Known => coverage.known += 1,
                    RepStatus::Learning | RepStatus::Due => coverage.learning += 1,
                    RepStatus::New => coverage.new += 1,
                }
            }
        }
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
        let mut seen = std::collections::HashSet::new();
        due.into_iter()
            .map(|(_, drill)| drill)
            .chain(new)
            .filter(|drill| seen.insert((Self::position_key(&drill.path), drill.answer.clone())))
            .collect()
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
                if node.children.is_empty()
                    && repertoire.is_user_move(path.len())
                    && repertoire
                        .moves_after(path)
                        .is_none_or(|moves| moves.is_empty())
                {
                    gaps.push(path.clone());
                }
                walk(repertoire, &node.children, path, gaps);
                path.pop();
            }
        }
        let mut gaps = Vec::new();
        walk(self, &self.roots, &mut Vec::new(), &mut gaps);
        let mut seen = std::collections::HashSet::new();
        gaps.retain(|path| seen.insert(Self::position_key(path)));
        gaps
    }

    /// Full identities are used as keys, so hash collisions cannot merge knowledge.
    pub fn position_index(&self) -> HashMap<String, Vec<(String, bool)>> {
        let mut index: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        for (key, _, node) in self.entries() {
            let moves = index.entry(key).or_default();
            if !moves.iter().any(|(san, _)| *san == node.san) {
                moves.push((node.san.clone(), moves.is_empty()));
            }
        }
        index
    }
}

fn identity(pos: &Chess) -> String {
    chess_core::shakmaty::fen::Fen::from_position(pos, chess_core::shakmaty::EnPassantMode::Legal)
        .to_string()
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
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
    fn position_index_finds_moves_by_full_identity() {
        let rep = sample();
        let index = rep.position_index();
        let start = identity(&Chess::default());
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
    fn transpositions_share_replies_cards_coverage_and_drills() {
        let mut rep = Repertoire::new("transpositions", true);
        rep.add_line(&["Nf3", "Nf6", "g3", "g6", "Bg2"]).unwrap();
        rep.add_line(&["g3", "g6", "Nf3", "Nf6", "Bg2"]).unwrap();
        let a = ["Nf3", "Nf6", "g3", "g6", "Bg2"];
        let b = ["g3", "g6", "Nf3", "Nf6", "Bg2"];
        rep.grade(&a, Grade::Easy, 10);
        assert_eq!(rep.node(&a).unwrap().card, rep.node(&b).unwrap().card);
        assert_eq!(rep.coverage(10).total(), 5);
        assert_eq!(
            rep.drills(21).iter().filter(|d| d.answer == "Bg2").count(),
            1
        );
        assert_eq!(rep.moves_after(&b[..4]).unwrap()[0].san, "Bg2");
        // A move order absent from the visible tree can still reach prepared knowledge.
        let mut other = Repertoire::new("other", true);
        other.add_line(&a).unwrap();
        assert_eq!(other.moves_after(&b[..4]).unwrap()[0].san, "Bg2");
    }

    #[test]
    fn review_submission_is_once_and_keeps_attempt_evidence() {
        let mut rep = sample();
        let path = vec!["e4".to_owned()];
        let attempt = crate::Attempt {
            failures: 1,
            hints: 2,
            reveals: 1,
            elapsed_ms: 3000,
        };
        assert!(rep.submit_review(1, &path, Grade::Easy, attempt.clone(), 10));
        assert!(!rep.submit_review(1, &path, Grade::Good, attempt, 10));
        assert_eq!(rep.history.len(), 1);
        assert_eq!(rep.history[0].applied, Grade::Again);
        assert_eq!(rep.history[0].requested, Grade::Easy);
        assert_eq!(rep.history[0].attempt.failures, 1);
        assert_eq!(rep.history[0].after.lapses, 1);
    }

    #[test]
    fn interrupted_session_round_trips_with_queue_and_evidence() {
        let mut rep = sample();
        let queue = rep.drills(0);
        rep.session = Some(crate::StudySession {
            queue: queue.clone(),
            finished: 2,
            total: 6,
            attempts: [(
                "position".to_owned(),
                crate::Attempt {
                    failures: 1,
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        });
        let restored: Repertoire =
            serde_json::from_slice(&serde_json::to_vec(&rep).unwrap()).unwrap();
        let session = restored.session.unwrap();
        assert_eq!(session.queue, queue);
        assert_eq!(session.finished, 2);
        assert_eq!(session.attempts["position"].failures, 1);
    }

    #[test]
    fn serde_round_trip() {
        let rep = sample();
        let text = serde_json::to_string(&rep).unwrap();
        let back: Repertoire = serde_json::from_str(&text).unwrap();
        assert_eq!(back.positions(), rep.positions());
    }
}
