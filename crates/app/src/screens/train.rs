//! Train: repertoire tree, spaced-repetition drill, and leaks from real games.

use chess_core::{Chess, Move, format_pv, parse_pgn, shakmaty::san::SanPlus};
use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription,
    WeakEntity, Window, div, prelude::*, px, rgb,
};
use gpui_component::input::{InputEvent, InputState};
use trainer::{Card, Drill, Grade, RepStatus};
use ui::{
    Arrow, BoardEvent, BoardView, MONO, T,
    widgets::{BtnKind, Listen, StyleExt, badge, btn, h_flex, mono, panel, section_label, v_flex},
};

use crate::{
    controls::{new_input, text_input},
    model::{AppModel, ModelEvent, ToastExt as _, pick_path, today},
    shell::Workspace,
};

mod drill;
mod leaks;
mod lines;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the user's move.
    Asking,
    /// The user played the prepared move.
    Correct,
    /// The user played something else and may retry.
    Wrong,
    /// The answer was revealed.
    Revealed,
}

pub struct TrainScreen {
    model: Entity<AppModel>,
    board: Entity<BoardView>,
    queue: Vec<Drill>,
    every_reply: bool,
    visible: bool,
    attempts: std::collections::HashMap<String, trainer::Attempt>,
    exercise_id: u64,
    attempt_tick: std::time::Instant,
    /// Drills finished in this session, counted into the header.
    finished: usize,
    total: usize,
    phase: Phase,
    message: Option<SharedString>,
    selected_path: Option<Vec<String>>,
    add_line: Entity<InputState>,
    adding: bool,
    _autosave: gpui::Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl TrainScreen {
    pub fn new(
        model: Entity<AppModel>,
        _workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let board = cx.new(|_| BoardView::new("train-board"));
        let prefs = model.read(cx).config.board.prefs();
        board.update(cx, |board, cx| board.set_prefs(prefs, cx));
        let add_line = new_input("", "1.e4 c5 2.Nf3 d6", window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&board, window, |this, _, event, _, cx| match event {
                BoardEvent::Moved(mv) => this.user_move(*mv, cx),
                BoardEvent::Square { .. } | BoardEvent::Premove(_) => {}
            }),
            cx.subscribe_in(&add_line, window, |this, state, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let text = state.read(cx).value().to_string();
                    this.submit_line(&text, window, cx);
                }
            }),
            cx.subscribe_in(&model, window, |this, model, event, _, cx| match event {
                ModelEvent::BoardPrefs => {
                    let prefs = model.read(cx).config.board.prefs();
                    this.board
                        .update(cx, |board, cx| board.set_prefs(prefs, cx));
                }
                ModelEvent::Repertoire => cx.notify(),
                ModelEvent::Restored => self::TrainScreen::resume_session(this, cx),
                _ => {}
            }),
        ];
        let mut this = Self {
            model,
            board,
            queue: Vec::new(),
            every_reply: false,
            visible: false,
            attempts: Default::default(),
            exercise_id: 0,
            attempt_tick: std::time::Instant::now(),
            finished: 0,
            total: 0,
            phase: Phase::Asking,
            message: None,
            selected_path: None,
            add_line,
            adding: false,
            _autosave: cx.spawn(async move |this: WeakEntity<Self>, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(10))
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            if !this.queue.is_empty() {
                                this.persist_session(cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }),
            _subscriptions: subscriptions,
        };
        this.resume_session(cx);
        this
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible && !visible {
            self.persist_session(cx);
        }
        self.visible = visible;
        self.attempt_tick = std::time::Instant::now();
    }

    fn resume_session(&mut self, cx: &mut Context<Self>) {
        let saved = self.model.read(cx).repertoire.session.clone();
        if let Some(saved) = saved {
            self.queue = saved.queue;
            self.finished = saved.finished;
            self.total = saved.total;
            self.every_reply = saved.every_reply;
            self.attempts = saved.attempts;
            self.start_drill(cx);
            self.phase = match saved.phase {
                1 => Phase::Correct,
                2 => Phase::Wrong,
                3 => Phase::Revealed,
                _ => Phase::Asking,
            };
            if matches!(self.phase, Phase::Correct | Phase::Revealed) {
                self.board
                    .update(cx, |board, cx| board.set_interactive(false, cx));
            }
            self.persist_session(cx);
        } else {
            self.attempts.clear();
            self.reload_queue(cx);
        }
    }

    fn reload_queue(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        self.queue = model.repertoire.drills(today());
        if !self.every_reply {
            let mut seen = std::collections::HashSet::new();
            self.queue
                .retain(|drill| seen.insert(trainer::Repertoire::position_key(&drill.path)));
        }
        // Demo mode opens mid-session, as in the design.
        self.finished = if model.demo { 6 } else { 0 };
        self.total = self.queue.len() + self.finished;
        self.start_drill(cx);
    }

    fn current(&self) -> Option<&Drill> {
        self.queue.first()
    }

    /// Replays the drill's path, returning the position and last move.
    fn position_for(path: &[String]) -> Option<(Chess, Option<Move>)> {
        let mut pos = Chess::default();
        let mut last = None;
        for san in path {
            let parsed: chess_core::shakmaty::san::San = san.parse().ok()?;
            let mv = parsed.to_move(&pos).ok()?;
            let _ = SanPlus::from_move_and_play_unchecked(&mut pos, mv);
            last = Some(mv);
        }
        Some((pos, last))
    }

    fn persist_session(&mut self, cx: &mut Context<Self>) {
        self.evidence();
        let session = trainer::StudySession {
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_secs()),
            queue: self.queue.clone(),
            finished: self.finished,
            total: self.total,
            every_reply: self.every_reply,
            attempts: self.attempts.clone(),
            phase: match self.phase {
                Phase::Asking => 0,
                Phase::Correct => 1,
                Phase::Wrong => 2,
                Phase::Revealed => 3,
            },
        };
        self.model.update(cx, |model, cx| {
            model.repertoire.session = Some(session);
            model.save_repertoire(cx);
        });
    }

    pub fn flip(&mut self, cx: &mut Context<Self>) {
        self.board.update(cx, |board, cx| {
            let flipped = board.is_flipped();
            board.set_flipped(!flipped, cx);
        });
    }

    fn render_center(
        &mut self,
        board_size: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let drill = self.current().cloned();
        let white = self.model.read(cx).repertoire.white;
        let side = if white {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        };
        let position = self.finished;
        let total = self.total.max(position);
        let fraction = if total == 0 {
            0.0
        } else {
            self.finished as f32 / total as f32
        };
        let context = drill.as_ref().map(|drill| {
            let start = drill.path.len().saturating_sub(2);
            let moves = format_pv(start as u32, &drill.path[start..]);
            if start > 0 {
                format!("…{moves}")
            } else {
                moves
            }
        });
        let has_drill = drill.is_some();
        v_flex()
            .flex_none()
            .w(board_size)
            .child(
                h_flex()
                    .h(px(22.))
                    .mt(px(20.))
                    .gap(px(8.))
                    .child(section_label(format!("{} {position}, {} {}", ui::i18n::t("Completed"), self.queue.len(), ui::i18n::t("remaining"))))
                    .child(badge(format!(
                        "{} due",
                        self.queue
                            .iter()
                            .filter(|d| {
                                let mut path = d.path.clone();
                                path.push(d.answer.clone());
                                self.model
                                    .read(cx)
                                    .repertoire
                                    .card_for(&path)
                                    .is_some_and(|c| c.is_due(today()))
                            })
                            .count()
                    )))
                    .child(badge(format!(
                        "{} new",
                        self.queue
                            .iter()
                            .filter(|d| {
                                let mut path = d.path.clone();
                                path.push(d.answer.clone());
                                self.model
                                    .read(cx)
                                    .repertoire
                                    .card_for(&path)
                                    .is_some_and(|c| c.state == trainer::CardState::New)
                            })
                            .count()
                    )))
                    .child(
                        div()
                            .flex_1()
                            .h(px(3.))
                            .rounded(px(1.))
                            .bg(rgb(T.selected))
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(fraction))
                                    .rounded(px(1.))
                                    .bg(rgb(T.accent)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .h(px(30.))
                    .mt(px(10.))
                    .mb(px(9.))
                    .gap(px(12.))
                    .items_baseline()
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if has_drill {
                                format!("Your move as {side}")
                            } else {
                                ui::i18n::t(if self.finished > 0 {
                                    "Session complete"
                                } else if self.model.read(cx).repertoire.positions() == 0 {
                                    "Your repertoire is empty"
                                } else {
                                    "Nothing scheduled today"
                                }).to_owned()
                            }),
                    )
                    .children(
                        context.map(|text| mono(text).text_as(14., T.muted)),
                    ),
            )
            .when(has_drill, |column| {
                column.child(div().size(board_size).flex_none().child(self.board.clone()))
            })
            .when(!has_drill, |column| column.child(
                div().mt(px(12.)).text_color(rgb(T.fg_2)).child(ui::i18n::t(
                    if self.finished > 0 {
                        "All session positions completed. Come back when more are due, or add a line."
                    } else if self.model.read(cx).repertoire.positions() == 0 {
                        "Use Add line or Import PGN to prepare your first positions."
                    } else {
                        "Come back when positions are due, or use Add line to learn something new."
                    }
                ))
            ))
            .when(!has_drill, |column| column.child(
                btn("train-add-line", "Add line").on_click(this.listen(move |screen, window, cx| {
                            screen.adding = true;
                            screen.add_line.update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        }))
            ))
    }
}

/// The drill board: as large as the window allows, since its controls sit in the
/// right column rather than under it.
fn board_size(window: &Window) -> gpui::Pixels {
    let viewport = window.viewport_size();
    ui::board::snap_board_size(
        (viewport.height - px(240.))
            .min(viewport.width - px(720.))
            .max(px(320.)),
        window.scale_factor(),
    )
}

impl Render for TrainScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let board_size = board_size(window);
        let mode = btn(
            "reply-mode",
            if self.every_reply {
                "Mode: every prepared reply"
            } else {
                "Mode: any prepared reply"
            },
        )
        .on_click(cx.entity().listen(move |this, _, cx| {
            this.every_reply = !this.every_reply;
            this.reload_queue(cx);
        }));
        let history = self.model.read(cx).repertoire.history.len();
        let session = v_flex().gap(px(6.)).mt(px(14.)).child(mode.full()).child(
            h_flex()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .text_as(12., T.muted)
                        .child(format!("{history} completed reviews")),
                )
                .child(
                    btn("restart-study", "Start fresh")
                        .compact()
                        .tooltip(ui::i18n::t(
                            "Discard this session and start the queue again",
                        ))
                        .on_click(cx.entity().listen(move |this, _, cx| {
                            this.attempts.clear();
                            this.reload_queue(cx);
                        })),
                ),
        );
        let tree = self.render_tree(session, cx);
        let center = self.render_center(board_size, cx);
        let drill = self.render_drill(cx);
        let leaks = self.render_leaks(cx);
        let side = v_flex().gap(px(16.)).children(drill).child(leaks);
        super::three_columns(tree, "train-center", center, "leaks-column", side)
    }
}
