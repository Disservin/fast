//! A second engine that analyses the position of a move clicked in Line preview, while
//! the main analysis keeps searching the game position.

use std::path::PathBuf;

use chess_core::{GameTree, Move, NodeId, Position};
use engine::{EngineConfig, EngineEvent, EngineHandle, EngineId, SearchRequest};
use gpui::{Context, Task};

use crate::{config::KibitzConfig, live::LiveLine};

/// What a running kibitz process was started with; a change starts a new one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Launch {
    engine: EngineId,
    path: PathBuf,
    threads: u32,
    hash_mb: u32,
}

/// The kibitz engine of one Analyse tab. Its process starts on first use and quits when
/// the tab closes.
#[derive(Default)]
pub struct Kibitz {
    handle: Option<EngineHandle>,
    launch: Option<Launch>,
    search_id: u64,
    pos: Option<chess_core::Chess>,
    /// The engine's name, for the Line preview row.
    pub name: String,
    /// The best line found so far for the previewed position.
    pub line: Option<LiveLine>,
    /// A search is running.
    pub running: bool,
    _reader: Option<Task<()>>,
}

impl Drop for Kibitz {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.quit();
        }
    }
}

/// The options the kibitz process starts with: the engine's own analysis options, one
/// line, and the kibitz's threads and hash where the engine offers them.
fn launch_options(config: &EngineConfig, kibitz: &KibitzConfig) -> Vec<(String, String)> {
    let config = crate::live::analysis_config(config);
    let offers = |name: &str| config.info.options.iter().any(|option| option.name == name);
    let mut options = config.options.clone();
    for (name, value) in [
        ("Threads", kibitz.threads.max(1).to_string()),
        ("Hash", kibitz.hash_mb.max(1).to_string()),
        ("MultiPV", "1".to_owned()),
    ] {
        if offers(name) {
            options.insert(name.to_owned(), value);
        }
    }
    options.into_iter().collect()
}

impl Kibitz {
    /// Analyses the position `extra` moves past `node`, starting or replacing the
    /// process when the engine or its settings changed.
    #[allow(clippy::too_many_arguments)]
    pub fn analyse(
        &mut self,
        config: &EngineConfig,
        settings: &KibitzConfig,
        tree: &GameTree,
        node: NodeId,
        extra: &[Move],
        game: u64,
        cx: &mut Context<Self>,
    ) {
        let launch = Launch {
            engine: config.id,
            path: config.path.clone(),
            threads: settings.threads,
            hash_mb: settings.hash_mb,
        };
        if self.launch.as_ref() != Some(&launch) || self.handle.is_none() {
            if let Some(handle) = self.handle.take() {
                handle.quit();
            }
            let handle = EngineHandle::spawn(
                config.path.clone(),
                Vec::new(),
                launch_options(config, settings),
            );
            self._reader = Some(self.spawn_reader(&handle, cx));
            self.handle = Some(handle);
            self.launch = Some(launch);
        }
        self.name = config.name.clone();
        let moves: Vec<String> = tree
            .path_to(node)
            .iter()
            .filter_map(|&id| tree.node(id).mv)
            .chain(extra.iter().copied())
            .map(|mv| tree.uci(mv))
            .collect();
        let mut pos = tree.node(node).pos.clone();
        for mv in extra {
            pos.play_unchecked(*mv);
        }
        self.line = None;
        self.search_id += 1;
        // Nothing to search once the game is over.
        self.running = !pos.is_game_over();
        let chess960 = pos.castles().mode() == chess_core::shakmaty::CastlingMode::Chess960;
        self.pos = Some(pos);
        if let Some(handle) = &self.handle {
            if self.running {
                handle.search(SearchRequest {
                    fen: tree.fen(GameTree::ROOT),
                    moves,
                    search_moves: Vec::new(),
                    limits: None,
                    search_id: self.search_id,
                    game,
                    chess960,
                });
            } else {
                handle.stop();
            }
        }
        cx.notify();
    }

    /// Stops the search and forgets its line; the process stays loaded for the next one.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = &self.handle {
            handle.stop();
        }
        self.search_id += 1;
        self.running = false;
        self.line = None;
        self.pos = None;
        cx.notify();
    }

    fn spawn_reader(&self, handle: &EngineHandle, cx: &mut Context<Self>) -> Task<()> {
        crate::event_reader::spawn_batched(
            handle.events(),
            Some(std::time::Duration::from_millis(100)),
            cx,
            Self::apply_events,
        )
    }

    fn apply_events(&mut self, batch: Vec<EngineEvent>, cx: &mut Context<Self>) {
        for event in batch {
            match event {
                EngineEvent::Analysis { search_id, update }
                    if search_id == self.search_id
                        && update.multipv == 1
                        && !update.bound
                        && !update.pv.is_empty() =>
                {
                    let Some(pos) = &self.pos else { continue };
                    self.line = Some(LiveLine::new(
                        pos,
                        update,
                        String::new(),
                        String::new(),
                        String::new(),
                    ));
                }
                EngineEvent::BestMove { search_id, .. } if search_id == self.search_id => {
                    self.running = false;
                }
                EngineEvent::Crashed(reason) => {
                    log::error!("kibitz engine {} stopped: {reason}", self.name);
                    self.handle = None;
                    self.launch = None;
                    self._reader = None;
                    self.running = false;
                }
                _ => {}
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{EngineInfo, OptionKind, UciOption};

    #[test]
    fn kibitz_options_limit_threads_and_hash_where_offered() {
        let spin = |name: &str| UciOption {
            name: name.to_owned(),
            kind: OptionKind::Spin {
                default: 1,
                min: 1,
                max: 1024,
            },
        };
        let mut config = EngineConfig {
            info: EngineInfo {
                options: vec![spin("Threads"), spin("Hash")],
                ..Default::default()
            },
            ..Default::default()
        };
        config.options.insert("Threads".to_owned(), "16".to_owned());
        config
            .options
            .insert("SyzygyPath".to_owned(), "/tb".to_owned());
        let options = launch_options(&config, &KibitzConfig::default());
        let get = |name: &str| {
            options
                .iter()
                .find(|(option, _)| option == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(get("Threads"), Some("1"));
        assert_eq!(get("Hash"), Some("16"));
        assert_eq!(get("SyzygyPath"), Some("/tb"));
        // Not offered by this engine, so not sent.
        assert_eq!(get("MultiPV"), None);
    }
}
