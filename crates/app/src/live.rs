//! Continuous multi-engine analysis of one position, throttled for the UI.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use chess_core::{GameTree, Move, NodeId, Position, position_hash, shakmaty::uci::UciMove};
use engine::{
    AnalysisUpdate, EngineConfig, EngineEvent, EngineHandle, EngineId, EngineLog, EngineStatus,
    Limits, LogKind, Score, SearchRequest, Wdl,
};
use gpui::{App, Context, Entity, EventEmitter, Task};

use crate::{
    config::AnalysisLimit,
    model::{AppModel, ConfigExt as _, ModelEvent, ToastExt as _, split_engine_name},
};

mod cache;
mod engines;
mod search;

pub use self::{cache::*, engines::*};

/// UI updates per engine are bounded to one per this interval (≤ 10 Hz).
const THROTTLE: Duration = Duration::from_millis(100);

/// A White-relative score as centipawns for graphs, mates at the edge.
pub fn score_cp(score: Score) -> i32 {
    match score {
        Score::Cp(cp) => cp,
        Score::Mate(mate) if mate > 0 => chess_core::annotation::MATE_CP,
        Score::Mate(_) => -chess_core::annotation::MATE_CP,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiveLine {
    pub update: AnalysisUpdate,
    /// White-relative score.
    pub score: Option<Score>,
    /// White-relative win/draw/loss.
    pub wdl: Option<Wdl>,
    /// PV converted to SAN from the analysed position.
    pub sans: Vec<String>,
    /// Remembered from an earlier visit to this position; replaced once the running
    /// search reaches the same depth.
    pub cached: bool,
    /// Cached from a different engine signature or search limit.
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    search_limit: String,
    #[serde(default)]
    engine_signature: String,
    #[serde(default)]
    position_context: String,
}

impl LiveLine {
    /// A fresh line from a search of `pos`, with its score and WDL made White-relative.
    pub(crate) fn new(
        pos: &chess_core::Chess,
        update: AnalysisUpdate,
        search_limit: String,
        engine_signature: String,
        position_context: String,
    ) -> Self {
        let white_to_move = pos.turn().is_white();
        Self {
            score: update
                .score
                .map(|score| if white_to_move { score } else { score.negate() }),
            wdl: update
                .wdl
                .map(|wdl| if white_to_move { wdl } else { wdl.flip() }),
            sans: chess_core::pv_to_san(pos, &update.pv),
            update,
            cached: false,
            stale: false,
            search_limit,
            engine_signature,
            position_context,
        }
    }
}

pub struct LiveEngine {
    pub config: EngineConfig,
    pub handle: Option<EngineHandle>,
    pub lines: BTreeMap<u32, LiveLine>,
    pub running: bool,
    /// Kept after a crash so the failure can be read.
    pub log: EngineLog,
    _reader: Option<Task<()>>,
}

impl LiveEngine {
    /// Latest numbers for the whole search, from the best line.
    pub fn stats(&self) -> Option<&AnalysisUpdate> {
        self.lines.get(&1).map(|line| &line.update)
    }
}

pub enum LiveEvent {
    Updated,
    /// An engine reached an evaluation for the analysed node (White-relative) at a
    /// depth.
    Eval(NodeId, Score, u32),
}

pub struct LiveAnalysis {
    model: Entity<AppModel>,
    pub engines: Vec<LiveEngine>,
    pub on: bool,
    active: bool,
    /// Searching is held: engines stay loaded and keep their last lines, but no
    /// search runs, even when the position changes, until resumed.
    pub paused: bool,
    search_id: u64,
    node: Option<NodeId>,
    pos: Option<chess_core::Chess>,
    hash: u64,
    /// The analysed position ends the game (mate, stalemate, a draw rule): nothing to
    /// search.
    pub end: Option<chess_core::GameEnd>,
    pub cache: LineCache,
    /// Replaces the configured search limit, e.g. a fixed node count per position
    /// while a game is analysed backwards.
    pub limit_override: Option<AnalysisLimit>,
    /// Bumped when Analyse loads a different game, so engines get `ucinewgame`.
    pub game: u64,
    fen: String,
    moves: Vec<String>,
    /// Root-move restrictions for this position only; never used as a full eval.
    pub search_moves: Vec<String>,
    _model_sub: gpui::Subscription,
}

impl EventEmitter<LiveEvent> for LiveAnalysis {}

impl LiveAnalysis {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        let on = model.read(cx).config.engine_on;
        let sub = cx.subscribe(&model, |this, _, event, cx| {
            if *event == ModelEvent::Engines {
                this.restart(cx);
            }
        });
        let mut this = Self {
            model,
            engines: Vec::new(),
            on,
            active: false,
            paused: false,
            search_id: 0,
            node: None,
            pos: None,
            hash: 0,
            end: None,
            cache: LineCache::default(),
            limit_override: None,
            game: 0,
            fen: String::new(),
            moves: Vec::new(),
            search_moves: Vec::new(),
            _model_sub: sub,
        };
        this.restart(cx);
        this
    }

    /// An engine is searching for this tab right now.
    pub fn searching(&self) -> bool {
        self.active
            && self.on
            && !self.paused
            && self
                .engines
                .iter()
                .any(|engine| engine.handle.is_some() && engine.running)
    }

    /// Engines with a live process.
    pub fn loaded(&self) -> usize {
        self.engines
            .iter()
            .filter(|engine| engine.handle.is_some())
            .count()
    }

    /// Every loaded engine has finished its search, and analysis is not paused.
    pub fn idle(&self) -> bool {
        !self.paused
            && self
                .engines
                .iter()
                .filter(|engine| engine.handle.is_some())
                .all(|engine| !engine.running)
    }

    /// Engine ids in table order, for cache lookups.
    pub fn engine_ids(&self) -> Vec<EngineId> {
        self.engines.iter().map(|engine| engine.config.id).collect()
    }

    /// Principal engine for the eval column and line preview.
    pub fn primary(&self) -> Option<&LiveEngine> {
        self.engines
            .iter()
            .find(|engine| !engine.lines.is_empty())
            .or_else(|| self.engines.first())
    }

    /// Prefer an engine with actual WDL data; retain an enabled source while waiting.
    pub fn wdl_engine(&self) -> Option<&LiveEngine> {
        let has_data = |engine: &&LiveEngine| engine.lines.values().any(|line| line.wdl.is_some());
        let visible = |engine: &&LiveEngine| wdl_visible(&engine.config, has_data(engine));
        self.engines
            .iter()
            .filter(visible)
            .find(has_data)
            .or_else(|| self.engines.iter().find(visible))
    }
}

/// Explicitly disabling the UCI option also hides cached WDL. Engines which
/// report WDL natively need no UCI_ShowWDL option.
pub fn wdl_visible(config: &EngineConfig, has_data: bool) -> bool {
    match config.options.get("UCI_ShowWDL").map(String::as_str) {
        Some("false") => false,
        Some("true") => true,
        _ => has_data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(depth: u32, cp: i32) -> BTreeMap<u32, LiveLine> {
        let line = LiveLine {
            update: AnalysisUpdate {
                depth,
                multipv: 1,
                ..Default::default()
            },
            score: Some(Score::Cp(cp)),
            position_context: "ctx".to_owned(),
            ..Default::default()
        };
        [(1, line)].into_iter().collect()
    }

    #[test]
    fn wdl_visibility_respects_option_and_native_output() {
        let mut config = EngineConfig::default();
        assert!(!wdl_visible(&config, false));
        assert!(wdl_visible(&config, true));
        config.options.insert("UCI_ShowWDL".into(), "true".into());
        assert!(wdl_visible(&config, false));
        config.options.insert("UCI_ShowWDL".into(), "false".into());
        assert!(!wdl_visible(&config, true));
    }

    #[test]
    fn cache_keeps_the_deepest_lines_and_marks_them() {
        let mut cache = LineCache::default();
        cache.store(1, 42, &lines(20, 30));
        cache.store(1, 42, &lines(12, -10));
        let kept = cache.get(1, 42).unwrap();
        assert_eq!(kept[&1].update.depth, 20);
        assert!(kept[&1].cached);
        cache.store(1, 42, &lines(25, 15));
        assert_eq!(cache.eval(&[1], 42, "ctx"), Some((15, 25)));
        // Another engine's analysis of the same position is separate.
        assert_eq!(cache.eval(&[2], 42, "ctx"), None);
        cache.store(2, 42, &lines(8, 50));
        assert_eq!(cache.eval(&[2, 1], 42, "ctx"), Some((50, 8)));
        cache.forget_engine(1);
        assert!(cache.get(1, 42).is_none());
        assert!(cache.get(2, 42).is_some());
    }

    #[test]
    fn analysis_mode_is_set_only_where_offered() {
        let mut config = EngineConfig::default();
        assert!(
            !analysis_config(&config)
                .options
                .contains_key("UCI_AnalyseMode")
        );
        config.info.options.push(engine::UciOption {
            name: "UCI_AnalyseMode".to_owned(),
            kind: engine::OptionKind::Check { default: false },
        });
        assert_eq!(
            analysis_config(&config)
                .options
                .get("UCI_AnalyseMode")
                .map(String::as_str),
            Some("true")
        );
        // The saved configuration is left alone.
        assert!(config.options.is_empty());
    }

    #[test]
    fn mates_map_to_the_graph_edge() {
        assert_eq!(score_cp(Score::Mate(3)), 2000);
        assert_eq!(score_cp(Score::Mate(-1)), -2000);
        assert_eq!(score_cp(Score::Cp(-37)), -37);
    }
}
