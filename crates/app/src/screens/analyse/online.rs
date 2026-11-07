//! Analyse's Online panel: the ChessTB Explorer tablebase and chessdb.cn for the
//! position on the board. It sends nothing while closed.

use std::{collections::HashMap, time::Duration};

use chess_core::{GameTree, Position, shakmaty::san::San};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, Task, Window, div, prelude::*, px, rgb,
};
use ui::{
    T,
    widgets::{h_flex, mono, panel, section_label, stat, v_flex},
};

use super::AnalyseScreen;
use crate::online::{
    CloudReply, Outcome, TABLEBASE_PIECES, TbProbe, cloud_score, probe_tablebase, query_chessdb,
};

/// Quick navigation only queries where it stops.
const DEBOUNCE: Duration = Duration::from_millis(300);
const CACHE_LIMIT: usize = 1000;
const MAX_ROWS: usize = 16;
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
        cx.notify();
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
                    cx.notify();
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
                    cx.notify();
                });
            }));
        }
    }

    /// Plays a move picked in the panel, unless a PV preview is showing another position.
    fn play_online_move(&mut self, san: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.pv_view.is_some() {
            return;
        }
        let pos = self.tree.node(self.current).pos.clone();
        let Some(mv) = San::from_ascii(san.as_bytes())
            .ok()
            .and_then(|san| san.to_move(&pos).ok())
        else {
            return;
        };
        self.play_move(mv, window, cx);
    }

    pub(super) fn render_online_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let white = self.analysed_pos().turn().is_white();
        panel()
            .flex_none()
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(40.))
                    .px(px(16.))
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(T.border))
                    .child(section_label("Online")),
            )
            .child(self.render_tablebase(white, cx))
            .child(div().h(px(1.)).bg(rgb(T.border)))
            .child(self.render_cloud(cx))
            .into_any_element()
    }

    fn render_tablebase(&self, white: bool, cx: &mut Context<Self>) -> Div {
        let section = v_flex().p(px(16.)).gap(px(8.));
        let probe = match &self.online.tablebase {
            Lookup::Ready(Some(probe)) => probe,
            Lookup::Ready(None) => {
                return section
                    .child(title("Tablebase", TABLEBASE_SOURCE))
                    .child(note("Not in the tablebase"));
            }
            other => {
                return section
                    .child(title("Tablebase", TABLEBASE_SOURCE))
                    .child(status(other));
            }
        };
        let side = |win: bool| if win == white { "White" } else { "Black" };
        let verdict = match probe.outcome() {
            Outcome::Win => format!("{} wins", side(true)),
            Outcome::CursedWin => format!("Draw by the 50-move rule ({} would win)", side(true)),
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
        let summary = h_flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                div()
                    .mr(px(4.))
                    .text_size(px(13.))
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
            );
        let rows = probe.rows();
        let more = rows.len().saturating_sub(MAX_ROWS);
        let mut table = v_flex().child(row_layout(
            header("Move"),
            header("Result"),
            header("DTZ"),
            header("DTM"),
        ));
        for (index, row) in rows.into_iter().take(MAX_ROWS).enumerate() {
            let color = outcome_color(row.outcome);
            let result = if row.is_mate {
                "Mate".to_owned()
            } else if let Some(reason) = &row.draw_reason {
                reason.replace('_', " ")
            } else {
                row.outcome.label().to_owned()
            };
            let distance = |d: Option<i32>| d.map_or("–".to_owned(), |d| d.abs().to_string());
            let san = row.san.clone();
            table = table.child(
                div()
                    .id(("tb-move", index))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(T.hover)))
                    .on_click(cx.listener(move |screen, _, window, cx| {
                        screen.play_online_move(&san, window, cx)
                    }))
                    .child(row_layout(
                        mono(row.san).font_weight(FontWeight::SEMIBOLD),
                        div().text_color(rgb(color)).child(result),
                        mono(distance(row.dtz)).text_color(rgb(T.fg_2)),
                        mono(distance(row.dtm)).text_color(rgb(T.fg_2)),
                    )),
            );
        }
        section
            .child(title("Tablebase", TABLEBASE_SOURCE))
            .child(summary)
            .child(table)
            .children((more > 0).then(|| note(format!("{more} more moves"))))
    }

    fn render_cloud(&self, cx: &mut Context<Self>) -> Div {
        let section = v_flex()
            .p(px(16.))
            .gap(px(8.))
            .child(title("Cloud database", "chessdb.cn"));
        let moves = match &self.online.cloud {
            Lookup::Ready(CloudReply::Moves(moves)) => moves,
            Lookup::Ready(CloudReply::Unknown) => {
                return section.child(note("Not analysed in the cloud database yet"));
            }
            Lookup::Ready(CloudReply::Over(reason)) => return section.child(note(reason.clone())),
            other => return section.child(status(other)),
        };
        if moves.is_empty() {
            return section.child(note("No moves"));
        }
        let more = moves.len().saturating_sub(MAX_ROWS);
        let mut table = v_flex().child(row_layout(
            header("Move"),
            header("Score"),
            header("Win %"),
            header("Note"),
        ));
        for (index, mv) in moves.iter().take(MAX_ROWS).enumerate() {
            let color = match mv.rank {
                2 => T.good,
                1 => T.fg,
                _ => T.fg_2,
            };
            let san = mv.san.clone();
            table = table.child(
                div()
                    .id(("cloud-move", index))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(T.hover)))
                    .on_click(cx.listener(move |screen, _, window, cx| {
                        screen.play_online_move(&san, window, cx)
                    }))
                    .child(row_layout(
                        mono(mv.san.clone())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(color)),
                        mono(cloud_score(mv.score)),
                        mono(mv.winrate.clone().unwrap_or_else(|| "–".into()))
                            .text_color(rgb(T.fg_2)),
                        mono(mv.note.clone()).text_color(rgb(T.muted)),
                    )),
            );
        }
        section
            .child(table)
            .children((more > 0).then(|| note(format!("{more} more moves"))))
    }
}

/// A section's name, with the service it comes from on the right.
fn title(name: &'static str, source: &'static str) -> Div {
    h_flex()
        .justify_between()
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(name),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(T.faint))
                .child(source),
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

fn header(text: &'static str) -> Div {
    div()
        .text_size(px(11.))
        .text_color(rgb(T.muted))
        .child(text)
}

fn row_layout(a: Div, b: Div, c: Div, d: Div) -> Div {
    h_flex()
        .h(px(24.))
        .px(px(4.))
        .gap(px(8.))
        .text_size(px(13.))
        .child(a.w(px(64.)).flex_none())
        .child(b.w(px(120.)).flex_none())
        .child(c.w(px(56.)).flex_none())
        .child(d.flex_1().min_w_0())
}

fn outcome_color(outcome: Outcome) -> u32 {
    match outcome {
        Outcome::Win => T.good,
        Outcome::Loss => T.q_blunder,
        Outcome::CursedWin | Outcome::BlessedLoss => T.warn,
        _ => T.fg_2,
    }
}
