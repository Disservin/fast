//! `--demo`: a separate, generated profile showing every screen populated, including
//! scripted engine output so the UI can be explored without any engine installed.
//! Nothing here runs in normal mode, and it never touches the real profile.

use std::{collections::HashMap, path::Path};

use chess_core::{
    Chess, GameTree, Position,
    shakmaty::{Move, san::SanPlus},
};
use db::{Database, ImportControl, ImportOptions, import_games};
use engine::{AnalysisUpdate, EngineConfig, EngineInfo, OptionKind, Score, UciOption, Wdl};
use trainer::{Card, CardState, CuratedRow, RepNode, Repertoire};

mod engines;
mod games;
mod repertoire;

pub use self::engines::*;
use self::{games::*, repertoire::*};

use crate::{
    config::{Config, Paths},
    model::today,
};

const LINE: &str = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4 Ne2 Ne8";
const ENGLISH_ATTACK: &str = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3";

/// A tiny deterministic generator so demo data is identical on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }
}

/// Builds the demo profile if needed and fills in the demo configuration.
pub fn prepare(paths: &Paths, config: &mut Config) {
    let today = today();
    config.player_name = "me".to_owned();
    config.saved_filters = vec![
        crate::config::SavedFilter {
            name: "Najdorf losses as White".to_owned(),
            player: "me".to_owned(),
            colour: 1,
            result: 3,
            eco: "B90–B99".to_owned(),
            ..Default::default()
        },
        crate::config::SavedFilter {
            name: "Annotated model games".to_owned(),
            only_annotated: true,
            ..Default::default()
        },
    ];
    // A remembered Lichess account, so Settings › Databases and Import show one.
    config.player_aliases = vec!["me_on_lichess".to_owned()];
    config.accounts = vec![crate::config::Account {
        site: crate::game_download::Site::Lichess,
        name: "me_on_lichess".to_owned(),
        database: paths.databases_dir().join("my_games.sqlite"),
        // 2026-11-02 18:00 UTC and the day after.
        last_game_ms: Some(1_793_642_400_000),
        updated_at: Some(1_793_700_000),
    }];
    config.player_rating = Some(2061);
    let opts = [
        ("Threads", "16"),
        ("Hash", "4096"),
        ("MultiPV", "3"),
        ("SyzygyPath", "D:\\Tablebases\\syzygy"),
        ("UCI_ShowWDL", "true"),
    ];
    config.engines = vec![
        demo_engine(
            1,
            (
                "Stockfish",
                "17.1",
                "C:\\Engines\\stockfish\\stockfish-avx2.exe",
            ),
            true,
            (true, true, true),
            &opts,
        ),
        demo_engine(
            2,
            ("Lc0", "0.31", "C:\\Engines\\lc0\\lc0.exe"),
            true,
            (true, false, false),
            &opts,
        ),
        demo_engine(
            3,
            ("Maia", "1900", "C:\\Engines\\maia\\lc0.exe"),
            true,
            (false, false, true),
            &opts,
        ),
        demo_engine(
            4,
            ("Berserk", "13", "C:\\Engines\\berserk\\berserk.exe"),
            false,
            (false, false, false),
            &opts,
        ),
    ];
    config.next_engine_id = 5;

    let databases = paths.databases_dir();
    let stamp = databases.join(".generated-v4");
    if stamp.exists() {
        return;
    }
    let _ = std::fs::remove_dir_all(&paths.data_dir);
    if let Err(err) = std::fs::create_dir_all(&databases) {
        log::error!("could not create demo directory: {err}");
        return;
    }
    let mut rng = Rng(0x0DD5_EED5_0000_0042);
    my_games(&databases.join("my_games.sqlite"), &mut rng);
    reference_db(&databases.join("a_reference.sqlite"), &mut rng);
    small_db(
        &databases.join("b_najdorf.sqlite"),
        "Najdorf model games",
        186,
        "Model",
        ENGLISH_ATTACK,
        &mut rng,
    );
    small_db(
        &databases.join("c_opponent.sqlite"),
        "Opponent: knightowl_42",
        74,
        "knightowl_42",
        "d4 Nf6 c4 e6",
        &mut rng,
    );
    if let Ok(text) = serde_json::to_string(&repertoire(today)) {
        let _ = std::fs::write(paths.repertoire_file(), text);
    }
    if let Ok(notes) = db::NoteStore::open(paths.notes_file()) {
        let tree = chess_core::sample::najdorf();
        let end = *tree.mainline().last().unwrap_or(&GameTree::ROOT);
        let _ = notes.set(
            chess_core::position_hash(&tree.node(end).pos),
            "Check f4 vs h4 move order — I keep playing h4 here.",
        );
    }
    let _ = std::fs::write(stamp, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_lines_are_legal() {
        let mut rng = Rng(7);
        for kind in [LINE, ENGLISH_ATTACK, PLAY_LINE] {
            assert_eq!(extend(kind, 0, &mut rng).len(), kind.split(' ').count());
        }
        assert_eq!(extend(ENGLISH_ATTACK, 40, &mut rng).len(), 40);
        let tree = chess_core::sample::najdorf();
        let end = *tree.mainline().last().unwrap();
        let hash = chess_core::position_hash(&tree.node(end).pos);
        let lines = &scripted_lines(hash)["Stockfish"];
        for line in lines {
            let sans = chess_core::pv_to_san(&tree.node(end).pos, &line.pv);
            assert_eq!(sans.len(), line.pv.len(), "illegal PV: {:?}", line.pv);
        }
    }

    #[test]
    fn repertoire_matches_the_design() {
        let today = 20_000;
        let rep = repertoire(today);
        assert_eq!(rep.positions(), 412);
        let rows = rep.tree_rows(today);
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[4].label, "12…b4 13.Ne2");
        assert_eq!(rows[4].status, trainer::RepStatus::Due);
        let drills = rep.drills(today);
        let due = drills
            .iter()
            .filter(|drill| {
                let path: Vec<&str> = drill
                    .path
                    .iter()
                    .map(String::as_str)
                    .chain([drill.answer.as_str()])
                    .collect();
                rep.node(&path)
                    .and_then(|node| node.card)
                    .is_some_and(|card| card.is_due(today))
            })
            .count();
        // With the 6 reviews already done, the session reads "7 of 24".
        assert_eq!(due, 18);
        assert_eq!(drills[0].answer, "Ne2");
        assert_eq!(drills[0].path.len(), 24);
    }
}
