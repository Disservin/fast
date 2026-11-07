//! Analyse's Online lookups: the ChessTB Explorer tablebase and chessdb.cn for the
//! position on the board, merged into Moves from here. It sends nothing while off.

use std::{collections::HashMap, time::Duration};

use chess_core::{GameTree, Position};
use gpui::{Context, Div, FontWeight, Task, div, prelude::*, px, rgb};
use ui::{
    T,
    widgets::{h_flex, stat},
};

use super::{AnalyseScreen, MoveRow};
use crate::online::{
    CloudReply, Outcome, TABLEBASE_PIECES, TbProbe, TbRow, cloud_score, probe_tablebase,
    query_chessdb,
};

/// Quick navigation only queries where it stops.
const DEBOUNCE: Duration = Duration::from_millis(300);
const CACHE_LIMIT: usize = 1000;
const TABLEBASE_SOURCE: &str = "chesstb-explorer.hf.space";

#[derive(Clone, Default)]
pub(super) enum Lookup<T> {
    #[default]
    Loading,
    Skipped(&'static str),
    Ready(T),
    Failed(String),
}

#[derive(Default)]
pub(super) struct OnlineState {
    pub open: bool,
    fen: Option<String>,
    tablebase: Lookup<Option<TbProbe>>,
    cloud: Lookup<CloudReply>,
    tb_cache: HashMap<String, Option<TbProbe>>,
    cloud_cache: HashMap<String, CloudReply>,
    _tb_task: Option<Task<()>>,
    _cloud_task: Option<Task<()>>,
}

fn remember<T>(cache: &mut HashMap<String, T>, fen: String, value: T) {
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(fen, value);
}

impl AnalyseScreen {
    pub(super) fn toggle_online(&mut self, cx: &mut Context<Self>) {
        self.online.open = !self.online.open;
        // Reopening looks the position up again, so failures can be retried.
        self.online.fen = None;
        if !self.online.open {
            self.online._tb_task = None;
            self.online._cloud_task = None;
        }
        // Closing drops the online columns' data at once; opening fills them in when
        // the next render looks the position up.
        self.rebuild_rows(cx);
    }

    /// Starts lookups when the panel is open and the board shows a new position.
    pub(super) fn sync_online(&mut self, cx: &mut Context<Self>) {
        if !self.online.open {
            return;
        }
        let pos = self.analysed_pos();
        let fen = GameTree::fen_of(&pos);
        if self.online.fen.as_ref() == Some(&fen) {
            return;
        }
        self.online.fen = Some(fen.clone());

        if pos.board().occupied().count() > TABLEBASE_PIECES {
            self.online.tablebase = Lookup::Skipped("Too many pieces for the tablebase");
            self.online._tb_task = None;
        } else if let Some(hit) = self.online.tb_cache.get(&fen) {
            self.online.tablebase = Lookup::Ready(hit.clone());
            self.online._tb_task = None;
        } else {
            self.online.tablebase = Lookup::Loading;
            let fen = fen.clone();
            self.online._tb_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(DEBOUNCE).await;
                let request = fen.clone();
                let result = cx
                    .background_spawn(async move { probe_tablebase(&request) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let state = &mut this.online;
                    if state.fen.as_ref() != Some(&fen) {
                        return;
                    }
                    state.tablebase = match result {
                        Ok(probe) => {
                            remember(&mut state.tb_cache, fen, probe.clone());
                            Lookup::Ready(probe)
                        }
                        Err(err) => Lookup::Failed(format!("{err:#}")),
                    };
                    this.rebuild_rows(cx);
                });
            }));
        }

        if let Some(hit) = self.online.cloud_cache.get(&fen) {
            self.online.cloud = Lookup::Ready(hit.clone());
            self.online._cloud_task = None;
        } else {
            self.online.cloud = Lookup::Loading;
            self.online._cloud_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(DEBOUNCE).await;
                let request = fen.clone();
                let result = cx
                    .background_spawn(async move { query_chessdb(&request) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let state = &mut this.online;
                    if state.fen.as_ref() != Some(&fen) {
                        return;
                    }
                    state.cloud = match result {
                        Ok(reply) => {
                            // An unknown position may be analysed later, so ask again next time.
                            if reply != CloudReply::Unknown {
                                remember(&mut state.cloud_cache, fen, reply.clone());
                            }
                            Lookup::Ready(reply)
                        }
                        Err(err) => Lookup::Failed(format!("{err:#}")),
                    };
                    this.rebuild_rows(cx);
                });
            }));
        }
        // Cached results, and the cleared columns of the last position, show at once.
        self.rebuild_rows(cx);
    }

    /// Adds the online moves for the position on the board to `rows`, onto the row of
    /// the same move where there is one. Results for another position are left out, so
    /// a lookup still running for the last position never shows here.
    pub(super) fn merge_online(&self, rows: &mut Vec<MoveRow>) {
        if !self.online.open {
            return;
        }
        let fen = GameTree::fen_of(&self.analysed_pos());
        if self.online.fen.as_ref() != Some(&fen) {
            return;
        }
        fn clean(san: &str) -> &str {
            san.trim_end_matches(['+', '#'])
        }
        let row_for = |rows: &mut Vec<MoveRow>, san: &str| -> usize {
            match rows.iter().position(|row| clean(&row.san) == clean(san)) {
                Some(index) => index,
                None => {
                    rows.push(MoveRow {
                        san: san.to_owned(),
                        ..Default::default()
                    });
                    rows.len() - 1
                }
            }
        };
        if let Lookup::Ready(Some(probe)) = &self.online.tablebase {
            for tb in probe.rows() {
                let index = row_for(rows, &tb.san);
                rows[index].tb = Some(tb);
            }
        }
        if let Lookup::Ready(CloudReply::Moves(moves)) = &self.online.cloud {
            for cloud in moves {
                let index = row_for(rows, &cloud.san);
                rows[index].cloud = Some(cloud.clone());
            }
        }
    }

    /// The tablebase verdict and the state of both lookups, shown above the moves.
    pub(super) fn render_online_status(&self) -> Option<Div> {
        if !self.online.open {
            return None;
        }
        let white = self.analysed_pos().turn().is_white();
        let tablebase = match &self.online.tablebase {
            Lookup::Ready(Some(probe)) => {
                let side = |win: bool| {
                    if win == white {
                        ui::i18n::t("White")
                    } else {
                        ui::i18n::t("Black")
                    }
                };
                let verdict = match probe.outcome() {
                    Outcome::Win => format!("{} wins", side(true)),
                    Outcome::CursedWin => {
                        format!("Draw by the 50-move rule ({} would win)", side(true))
                    }
                    Outcome::BlessedLoss => {
                        format!("Draw by the 50-move rule ({} would win)", side(false))
                    }
                    Outcome::Loss => format!("{} wins", side(false)),
                    _ => match &probe.draw_reason {
                        Some(reason) => format!("Draw ({})", reason.replace('_', " ")),
                        None => "Draw".into(),
                    },
                };
                let decided = probe.wdl != 0;
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(outcome_color(probe.outcome())))
                            .child(verdict),
                    )
                    .children(
                        probe
                            .dtz
                            .filter(|_| decided)
                            .map(|dtz| stat("DTZ", dtz.abs().to_string())),
                    )
                    .children(
                        probe
                            .dtm
                            .filter(|_| decided)
                            .map(|dtm| stat("DTM", dtm.abs().to_string())),
                    )
            }
            Lookup::Ready(None) => h_flex().child(note("Not in the tablebase")),
            other => h_flex().child(status(other)),
        };
        let cloud = match &self.online.cloud {
            Lookup::Ready(CloudReply::Moves(moves)) if moves.is_empty() => note("No moves"),
            Lookup::Ready(CloudReply::Moves(moves)) => note(format!(
                "{} move{}",
                moves.len(),
                if moves.len() == 1 { "" } else { "s" }
            )),
            Lookup::Ready(CloudReply::Unknown) => note("Not analysed yet"),
            Lookup::Ready(CloudReply::Over(reason)) => note(reason.clone()),
            other => status(other),
        };
        Some(
            h_flex()
                .flex_wrap()
                .gap_x(px(20.))
                .gap_y(px(4.))
                .px(px(16.))
                .py(px(8.))
                .border_t_1()
                .border_color(rgb(T.border))
                .text_size(px(12.))
                .child(source("Tablebase", TABLEBASE_SOURCE).child(tablebase))
                .child(source("Cloud", "chessdb.cn").child(cloud)),
        )
    }
}

/// The Tablebase column: the result the move leads to, with its distance.
pub(super) fn tb_cell(tb: &TbRow) -> (String, u32) {
    let result = if tb.is_mate {
        "Mate".to_owned()
    } else if let Some(reason) = &tb.draw_reason {
        reason.replace('_', " ")
    } else {
        tb.outcome.label().to_owned()
    };
    let distance = tb
        .dtm
        .or(tb.dtz)
        .filter(|_| !tb.is_mate && tb.outcome != Outcome::Draw);
    let text = match distance {
        Some(plies) => format!("{result} {}", plies.abs()),
        None => result,
    };
    (text, outcome_color(tb.outcome))
}

/// The Cloud column: chessdb.cn's score, brightest for its best moves.
pub(super) fn cloud_cell(cloud: &crate::online::CloudMove) -> (String, u32) {
    let color = match cloud.rank {
        2 => T.good,
        1 => T.fg_2,
        _ => T.muted,
    };
    (cloud_score(cloud.score), color)
}

/// Better results for the side to move rank higher.
pub(super) fn outcome_rank(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Win => 4,
        Outcome::CursedWin => 3,
        Outcome::Draw => 2,
        Outcome::BlessedLoss => 1,
        Outcome::Loss => 0,
        _ => -1,
    }
}

/// A lookup's name and service, then what it found.
fn source(name: &'static str, service: &'static str) -> Div {
    h_flex()
        .gap(px(8.))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(T.fg_2))
                .child(name),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(T.faint))
                .child(service),
        )
}

fn note(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .text_color(rgb(T.muted))
        .child(text.into())
}

fn status<T>(lookup: &Lookup<T>) -> Div {
    match lookup {
        Lookup::Loading => note("Looking up…"),
        Lookup::Skipped(reason) => note(*reason),
        Lookup::Failed(err) => note(err.clone()).text_color(rgb(T.warn)),
        Lookup::Ready(_) => note(""),
    }
}

fn outcome_color(outcome: Outcome) -> u32 {
    match outcome {
        Outcome::Win => T.good,
        Outcome::Loss => T.q_blunder,
        Outcome::CursedWin | Outcome::BlessedLoss => T.warn,
        _ => T.fg_2,
    }
}
