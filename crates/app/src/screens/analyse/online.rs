//! Analyse's Online lookups: the ChessTB Explorer tablebase and chessdb.cn for the
//! position on the board, shown in the Engines panel. It sends nothing while off.

use std::{collections::HashMap, time::Duration};

use chess_core::{GameTree, Position};
use gpui::{Context, Div, Task, div, prelude::*, px, rgb};
use ui::{
    T,
    widgets::{Listen, StyleExt, btn, h_flex, mono, v_flex},
};

use super::{AnalyseScreen, MoveRow};
use crate::online::{
    CloudReply, Outcome, TABLEBASE_PIECES, TbProbe, cloud_score, probe_tablebase, query_chessdb,
};

/// Quick navigation only queries where it stops.
const DEBOUNCE: Duration = Duration::from_millis(300);
const CACHE_LIMIT: usize = 1000;

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
    pub(super) fn merge_online(&self, rows: &mut Vec<MoveRow>, all_moves: bool) {
        if !self.online.open {
            return;
        }
        let fen = GameTree::fen_of(&self.analysed_pos());
        if self.online.fen.as_ref() != Some(&fen) {
            return;
        }
        if let Lookup::Ready(Some(probe)) = &self.online.tablebase {
            for tb in probe.rows() {
                for row in super::rows_for(rows, &tb.san, all_moves) {
                    row.tb = Some(tb.clone());
                }
            }
        }
        if let Lookup::Ready(CloudReply::Moves(moves)) = &self.online.cloud {
            for cloud in moves {
                for row in super::rows_for(rows, &cloud.san, all_moves) {
                    row.cloud = Some(cloud.clone());
                }
            }
        }
    }
}

impl AnalyseScreen {
    /// The Engines panel's footer while Online is on: chessdb.cn's best move and
    /// score, the tablebase's verdict (or how many pieces it needs), and a button to
    /// list every cloud move.
    pub(super) fn render_online_footer(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.online.open {
            return None;
        }
        let this = cx.entity();
        let white = self.analysed_pos().turn().is_white();
        let pieces = self.analysed_pos().board().occupied().count();
        let cloud = match &self.online.cloud {
            Lookup::Ready(CloudReply::Moves(moves)) => match moves.first() {
                Some(best) => {
                    let (score, colour) = cloud_cell(best, white);
                    h_flex()
                        .gap(px(6.))
                        .child(mono(score).text_color(rgb(colour)))
                        .child(mono(best.san.clone()).text_color(rgb(T.fg_2)))
                }
                None => h_flex().child(note("No moves")),
            },
            Lookup::Ready(CloudReply::Unknown) => h_flex().child(note("Not analysed yet")),
            Lookup::Ready(CloudReply::Over(reason)) => h_flex().child(note(reason.clone())),
            other => h_flex().child(status(other)),
        };
        let tablebase = match &self.online.tablebase {
            Lookup::Ready(Some(probe)) => {
                let (text, colour) = tb_verdict(probe, white);
                div().text_color(rgb(colour)).child(text)
            }
            Lookup::Ready(None) => note("Not in the tablebase"),
            Lookup::Skipped(_) => note(ui::i18n::t("{} pieces").replace("{}", &pieces.to_string())),
            other => status(other),
        };
        let cloud_moves = match &self.online.cloud {
            Lookup::Ready(CloudReply::Moves(moves)) => moves.clone(),
            _ => Vec::new(),
        };
        let tb_tip = ui::i18n::t("The tablebase knows positions with up to {} pieces")
            .replace("{}", &TABLEBASE_PIECES.to_string());
        let mut footer = v_flex().border_t_as(T.border).child(
            h_flex()
                .flex_wrap()
                .gap_x(px(16.))
                .gap_y(px(4.))
                .px(px(16.))
                .py(px(8.))
                .text_size(px(12.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(ui::widgets::icon("icons/cloud.svg", px(14.), T.muted))
                        .child(div().text_color(rgb(T.muted)).child("ChessDB"))
                        .child(cloud),
                )
                .child(
                    h_flex()
                        .id("tablebase-status")
                        .gap(px(6.))
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tb_tip.clone()).build(window, cx)
                        })
                        .child(
                            div()
                                .text_color(rgb(T.muted))
                                .child(ui::i18n::t("Tablebase")),
                        )
                        .child(tablebase),
                )
                .child(div().flex_1())
                .when(!cloud_moves.is_empty(), |row| {
                    row.child(
                        btn("all-cloud-moves", "All cloud moves")
                            .mini()
                            .kind(ui::widgets::BtnKind::Ghost)
                            .color(T.accent)
                            .selected(self.online_all)
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.online_all = !screen.online_all;
                                cx.notify();
                            })),
                    )
                }),
        );
        if self.online_all && !cloud_moves.is_empty() {
            let mut list = h_flex().flex_wrap().gap(px(4.)).px(px(16.)).pb(px(10.));
            for (index, cloud) in cloud_moves.into_iter().enumerate() {
                let (score, _) = cloud_cell(&cloud, white);
                let san = cloud.san.clone();
                list = list.child(
                    btn(("cloud-move", index), format!("{} {score}", cloud.san))
                        .mini()
                        .mono()
                        .pad(6.)
                        .tooltip("Play this move")
                        .on_click(this.listen(move |screen, window, cx| {
                            if let Some(&node) = screen.add_line(std::slice::from_ref(&san)).last()
                            {
                                screen.jump(node, window, cx);
                            }
                        })),
                );
            }
            footer = footer.child(list);
        }
        Some(footer)
    }
}

/// The tablebase's verdict on the position, from White's or Black's side, in its colour.
fn tb_verdict(probe: &TbProbe, white: bool) -> (String, u32) {
    let side = |win: bool| {
        if win == white {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        }
    };
    let verdict = match probe.outcome() {
        Outcome::Win => ui::i18n::t("{} wins").replace("{}", side(true)),
        Outcome::Loss => ui::i18n::t("{} wins").replace("{}", side(false)),
        Outcome::CursedWin | Outcome::BlessedLoss => {
            ui::i18n::t("Draw by the 50-move rule").to_owned()
        }
        _ => match &probe.draw_reason {
            Some(reason) => format!("{} ({})", ui::i18n::t("Draw"), reason.replace('_', " ")),
            None => ui::i18n::t("Draw").to_owned(),
        },
    };
    let distance = probe
        .dtm
        .or(probe.dtz)
        .filter(|_| probe.wdl != 0)
        .map(|plies| format!(" ({})", plies.abs()))
        .unwrap_or_default();
    (
        format!("{verdict}{distance}"),
        outcome_color(probe.outcome()),
    )
}

/// The Cloud column: chessdb.cn's score from White's side, brightest for its best moves.
pub(super) fn cloud_cell(cloud: &crate::online::CloudMove, white_to_move: bool) -> (String, u32) {
    let color = match cloud.rank {
        2 => T.good,
        1 => T.fg_2,
        _ => T.muted,
    };
    // chessdb.cn scores for the side to move; the table shows evals for White.
    let score = if white_to_move {
        cloud.score
    } else {
        -cloud.score
    };
    (cloud_score(score), color)
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

fn note(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_as(13., T.muted)
        .child(ui::i18n::t(&text.into()).to_owned())
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
