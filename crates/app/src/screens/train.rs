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
    widgets::{BtnKind, badge, btn, h_flex, mono, panel, section_label, v_flex},
};

use crate::{
    controls::{new_input, text_input},
    model::{AppModel, ModelEvent, pick_path, today},
    shell::Workspace,
};

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

/// Parses a typed line such as "1.e4 c5 2.Nf3" into plain SAN tokens.
pub fn parse_line_text(text: &str) -> Vec<String> {
    text.split_ascii_whitespace()
        .filter_map(|token| {
            let san = token.rsplit(['.', '…']).next().unwrap_or(token).trim();
            (!san.is_empty()).then(|| san.to_owned())
        })
        .collect()
}

/// Every line of a game tree as SAN paths, one per leaf.
fn tree_lines(tree: &chess_core::GameTree) -> Vec<Vec<String>> {
    let mut lines = Vec::new();
    let mut stack = vec![chess_core::GameTree::ROOT];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        if node.children.is_empty() && id != chess_core::GameTree::ROOT {
            lines.push(
                tree.path_to(id)
                    .iter()
                    .map(|&step| tree.node(step).san.clone())
                    .collect(),
            );
        }
        stack.extend(node.children.iter().rev());
    }
    lines
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

    fn attempt_key(&self) -> String {
        self.current()
            .map(|drill| {
                format!(
                    "{}:{}",
                    trainer::Repertoire::position_key(&drill.path).unwrap_or_default(),
                    drill.answer
                )
            })
            .unwrap_or_default()
    }

    fn evidence(&mut self) -> &mut trainer::Attempt {
        let elapsed = if self.visible {
            self.attempt_tick
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64
        } else {
            0
        };
        self.attempt_tick = std::time::Instant::now();
        let key = self.attempt_key();
        let attempt = self.attempts.entry(key).or_default();
        attempt.elapsed_ms = attempt.elapsed_ms.saturating_add(elapsed);
        attempt
    }

    fn start_drill(&mut self, cx: &mut Context<Self>) {
        self.exercise_id = self
            .model
            .read(cx)
            .repertoire
            .history
            .last()
            .map_or(1, |event| event.id + 1);
        self.attempt_tick = std::time::Instant::now();
        self.phase = Phase::Asking;
        self.message = None;
        let white = self.model.read(cx).repertoire.white;
        let color = if white {
            chess_core::Color::White
        } else {
            chess_core::Color::Black
        };
        let drill = self.current().cloned();
        self.selected_path = drill.as_ref().map(|drill| {
            drill
                .path
                .iter()
                .cloned()
                .chain([drill.answer.clone()])
                .collect()
        });
        let (pos, _) = drill
            .as_ref()
            .and_then(|drill| Self::position_for(&drill.path))
            .unwrap_or_else(|| (Chess::default(), None));
        self.board.update(cx, |board, cx| {
            board.set_position(pos, None, false, cx);
            board.set_arrows(Vec::new(), cx);
            board.set_player(Some(color), cx);
            board.set_flipped(!white, cx);
            board.set_interactive(drill.is_some(), cx);
        });
        self.persist_session(cx);
        cx.notify();
    }

    fn user_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        let Some(drill) = self.current().cloned() else {
            return;
        };
        if self.phase != Phase::Asking && self.phase != Phase::Wrong {
            return;
        }
        let Some((pos, _)) = Self::position_for(&drill.path) else {
            return;
        };
        let mut after = pos.clone();
        let played = SanPlus::from_move_and_play_unchecked(&mut after, mv).to_string();
        let clean = played.trim_end_matches(['+', '#']);
        let prepared = self
            .model
            .read(cx)
            .repertoire
            .moves_after(&drill.path)
            .is_some_and(|moves| moves.iter().any(|node| node.san == clean));
        if clean == drill.answer || (prepared && !self.every_reply) {
            if clean != drill.answer {
                self.evidence();
                let old_key = self.attempt_key();
                let evidence = self.attempts.remove(&old_key).unwrap_or_default();
                self.queue[0].answer = clean.to_owned();
                self.attempts.insert(self.attempt_key(), evidence);
            }
            self.phase = Phase::Correct;
            self.message = Some(format!("Correct: {played}. How well did you know it?").into());
            let last = mv.from().map(|from| (from, mv.to()));
            let volume = self.model.read(cx).config.board.sound_level();
            crate::sound::play_move(
                self.model.read(cx).config.board.sound_profile,
                volume,
                &mv,
                &after,
                None,
            );
            self.board.update(cx, |board, cx| {
                board.set_position(after, last, true, cx);
                board.set_interactive(false, cx);
            });
        } else {
            if !prepared {
                self.evidence().failures += 1;
            }
            self.phase = Phase::Wrong;
            self.message = Some(if prepared {
                format!("{played} is a valid repertoire move. This exercise asks for another prepared reply. Try again.").into()
            } else {
                format!("{played} is not in your repertoire here. Try again.").into()
            });
        }
        self.persist_session(cx);
        cx.notify();
    }

    fn answer_move(&self) -> Option<Move> {
        let drill = self.current()?;
        let (pos, _) = Self::position_for(&drill.path)?;
        let parsed: chess_core::shakmaty::san::San = drill.answer.parse().ok()?;
        parsed.to_move(&pos).ok()
    }

    fn hint(&mut self, cx: &mut Context<Self>) {
        let Some(mv) = self.answer_move() else { return };
        self.evidence().hints += 1;
        let role = mv.role();
        let name = match role {
            chess_core::Role::Pawn => "a pawn",
            chess_core::Role::Knight => "a knight",
            chess_core::Role::Bishop => "a bishop",
            chess_core::Role::Rook => "a rook",
            chess_core::Role::Queen => "the queen",
            chess_core::Role::King => "the king",
        };
        self.message = Some(format!("Hint: move {name}.").into());
        self.persist_session(cx);
        cx.notify();
    }

    fn show_answer(&mut self, cx: &mut Context<Self>) {
        let Some(mv) = self.answer_move() else { return };
        let Some(from) = mv.from() else { return };
        self.evidence().reveals += 1;
        self.phase = Phase::Revealed;
        self.message = Some(ui::i18n::t("Answer shown. Grade how well you knew it.").into());
        self.board.update(cx, |board, cx| {
            board.set_arrows(vec![Arrow::new(from, mv.to())], cx);
            board.set_interactive(false, cx);
        });
        self.persist_session(cx);
        cx.notify();
    }

    fn why(&mut self, cx: &mut Context<Self>) {
        let note = self
            .current()
            .map(|drill| drill.note.clone())
            .unwrap_or_default();
        self.message = Some(if note.is_empty() {
            ui::i18n::t("No note saved for this move yet.").into()
        } else {
            note.into()
        });
        cx.notify();
    }

    fn skip(&mut self, cx: &mut Context<Self>) {
        self.evidence();
        if self.queue.len() > 1 {
            let drill = self.queue.remove(0);
            self.queue.push(drill);
        }
        self.start_drill(cx);
    }

    fn grade(&mut self, grade: Grade, cx: &mut Context<Self>) {
        let Some(drill) = self.current().cloned() else {
            return;
        };
        if !matches!(self.phase, Phase::Correct | Phase::Revealed) {
            return;
        }
        let attempt = self.evidence().clone();
        let applied = attempt.scheduling_grade(grade);
        let attempt_key = self.attempt_key();
        let id = self.exercise_id;
        let path: Vec<String> = drill
            .path
            .iter()
            .cloned()
            .chain([drill.answer.clone()])
            .collect();
        let submitted = self.model.update(cx, |model, _| {
            model
                .repertoire
                .submit_review(id, &path, grade, attempt, today())
        });
        if !submitted {
            return;
        }
        self.attempts.remove(&attempt_key);
        self.queue.remove(0);
        // "Again" returns the card to the end of today's session.
        if applied == Grade::Again {
            self.queue.push(drill);
        } else {
            self.finished += 1;
        }
        self.start_drill(cx);
    }

    fn drill_leaks(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let drills: Vec<Drill> = model
            .leaks
            .iter()
            .filter_map(|leak| {
                Some(Drill {
                    path: leak.path.clone(),
                    answer: leak.prepared.clone()?,
                    note: format!(
                        "You played {} here in {} games; the repertoire answer is prepared.",
                        leak.played, leak.games
                    ),
                })
            })
            .collect();
        if drills.is_empty() {
            self.message = Some(ui::i18n::t("No leaks to drill.").into());
            cx.notify();
            return;
        }
        self.finished = 0;
        self.total = drills.len();
        self.queue = drills;
        self.start_drill(cx);
    }

    fn submit_line(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let line = parse_line_text(text);
        let refs: Vec<&str> = line.iter().map(String::as_str).collect();
        let result = self.model.update(cx, |model, cx| {
            let result = model.repertoire.add_line(&refs);
            if result.is_ok() {
                model.save_repertoire(cx);
                model.refresh_leaks(cx);
            }
            result
        });
        match result {
            Ok(()) => {
                self.adding = false;
                self.add_line
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.reload_queue(cx);
            }
            Err(err) => self.toast(format!("Could not add the line: {err}"), cx),
        }
    }

    fn find_gaps(&mut self, cx: &mut Context<Self>) {
        let gaps = self.model.read(cx).repertoire.gaps();
        let text = match gaps.first() {
            None => ui::i18n::t("No gaps: every line ends on an answered move.").to_owned(),
            Some(first) => format!(
                "{} positions need an answer, for example after {}.",
                gaps.len(),
                format_pv(
                    first.len().saturating_sub(4) as u32,
                    &first[first.len().saturating_sub(4)..]
                )
            ),
        };
        self.toast(text, cx);
    }

    fn toast(&self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.model.update(cx, |model, cx| model.toast(text, cx));
    }

    pub fn flip(&mut self, cx: &mut Context<Self>) {
        self.board.update(cx, |board, cx| {
            let flipped = board.is_flipped();
            board.set_flipped(!flipped, cx);
        });
    }

    /// Imports lines from a PGN into the active repertoire (variations included).
    pub fn prompt_open(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Add lines from PGN",
            None,
            cx,
            move |path, cx| {
                let Some(this) = this.upgrade() else { return };
                this.update(cx, |this, cx| {
                    this.add_lines_from(path, cx);
                });
            },
        );
    }

    fn add_lines_from(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let read = std::fs::read_to_string(&path);
        let message = match read
            .map_err(|err| format!("Could not read the file: {err}"))
            .and_then(|text| {
                parse_pgn(&text).map_err(|err| format!("Could not read the PGN: {err}"))
            }) {
            Ok(games) => {
                let mut added = 0;
                self.model.update(cx, |model, cx| {
                    for game in &games {
                        for line in tree_lines(game) {
                            let refs: Vec<&str> = line.iter().map(String::as_str).collect();
                            if model.repertoire.add_line(&refs).is_ok() {
                                added += 1;
                            }
                        }
                    }
                    model.save_repertoire(cx);
                    model.refresh_leaks(cx);
                });
                self.reload_queue(cx);
                format!("Added {added} lines to the repertoire.")
            }
            Err(message) => message,
        };
        self.toast(message, cx);
    }

    fn render_tree(
        &mut self,
        session: gpui::Div,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let today = today();
        let model = self.model.read(cx);
        let rep = &model.repertoire;
        let coverage = rep.coverage(today);
        let (known, learning, new) = coverage.percents();
        let rows = rep.tree_rows(today);
        let selected = self.selected_path.clone();
        let mut list = v_flex().mb(px(14.));
        for (index, row) in rows.into_iter().enumerate() {
            let is_selected = selected.as_ref().is_some_and(|path| {
                path.len() > row.path.len()
                    && path.starts_with(&row.path)
                    && row.status == RepStatus::Due
            });
            let (tag, color) = match row.status {
                RepStatus::Known => ("Known", T.good),
                RepStatus::Learning => ("Learning", T.fg_2),
                RepStatus::Due => ("Due", T.warn),
                RepStatus::New => ("New", T.muted),
            };
            list = list.child(
                h_flex()
                    .id(("rep-row", index))
                    .relative()
                    .h(px(32.))
                    .flex_none()
                    .pl(px(18. + row.depth as f32 * 14.))
                    .pr(px(18.))
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .when(is_selected, |row_el| row_el.bg(rgb(T.selected)))
                    .when(is_selected, |row_el| {
                        row_el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(rgb(T.accent)),
                        )
                    })
                    .child(mono(row.label).text_size(px(14.)).text_color(rgb(T.fg)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(color))
                            .child(tag),
                    ),
            );
        }
        let segment = |value: u32, color: u32| {
            div()
                .h_full()
                .flex_grow(value.max(1) as f32)
                .flex_basis(px(0.))
                .bg(rgb(color))
        };
        let add_this = this.clone();
        let gaps_this = this.clone();
        v_flex()
            .w(px(300.))
            .h_full()
            .flex_none()
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                v_flex()
                    .flex_none()
                    .px(px(18.))
                    .pt(px(16.))
                    .pb(px(10.))
                    .child(section_label(ui::i18n::t("Repertoire")).mb(px(9.)))
                    .child(
                        h_flex()
                            .justify_between()
                            .mb(px(14.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(rep.name.clone())),
                            )
                            .child(
                                mono(format!("{} pos.", rep.positions()))
                                    .text_size(px(14.))
                                    .text_color(rgb(T.muted)),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(6.))
                            .w_full()
                            .overflow_hidden()
                            .child(segment(known, T.fg))
                            .child(segment(learning, 0x9A948A))
                            .child(segment(new, T.bg)),
                    )
                    .child(
                        h_flex()
                            .mt(px(6.))
                            .gap(px(14.))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(format!("{known}% known"))
                            .child(format!("{learning}% learning"))
                            .child(format!("{new}% new")),
                    )
                    .child(session),
            )
            .child(
                div()
                    .id("rep-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list)
                    .when(self.adding, |column| {
                        column.child(
                            div()
                                .px(px(18.))
                                .pb(px(8.))
                                .child(text_input(&self.add_line)),
                        )
                    })
                    .child(
                        h_flex()
                            .px(px(18.))
                            .gap(px(6.))
                            .child(btn("add-line", "Add line").on_click(move |window, cx| {
                                add_this.update(cx, |screen, cx| {
                                    screen.adding = !screen.adding;
                                    if screen.adding {
                                        screen.add_line.update(cx, |input, cx| {
                                            input.focus(window, cx);
                                        });
                                    }
                                    cx.notify();
                                });
                            }))
                            .child(btn("find-gaps", "Find gaps").on_click(move |_, cx| {
                                gaps_this.update(cx, |screen, cx| screen.find_gaps(cx));
                            })),
                    ),
            )
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
        let position = self.finished + usize::from(drill.is_some());
        let total = self.total.max(position);
        let fraction = if total == 0 {
            0.0
        } else {
            position as f32 / total as f32
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
        let card: Card = drill
            .as_ref()
            .and_then(|drill| {
                let path: Vec<&str> = drill
                    .path
                    .iter()
                    .map(String::as_str)
                    .chain([drill.answer.as_str()])
                    .collect();
                self.model.read(cx).repertoire.card_for(&path)
            })
            .unwrap_or_default();
        let intervals = card.intervals();
        let has_drill = drill.is_some();
        let tool = |id: &'static str,
                    label: &'static str,
                    action: fn(&mut Self, &mut Context<Self>),
                    this: Entity<Self>| {
            btn(id, label).disabled(!has_drill).on_click(move |_, cx| {
                this.update(cx, action);
            })
        };
        let recall =
            |id: &'static str, label: String, grade: Grade, kind: BtnKind, this: Entity<Self>| {
                btn(id, label)
                    .flex_1()
                    .kind(kind)
                    .disabled(!has_drill)
                    .on_click(move |_, cx| this.update(cx, |screen, cx| screen.grade(grade, cx)))
            };
        v_flex()
            .flex_none()
            .w(board_size)
            .child(
                h_flex()
                    .h(px(22.))
                    .mt(px(20.))
                    .gap(px(8.))
                    .child(section_label(format!("Drill {position} of {total}")))
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
                                ui::i18n::t("Nothing due").to_owned()
                            }),
                    )
                    .children(
                        context.map(|text| mono(text).text_size(px(14.)).text_color(rgb(T.muted))),
                    ),
            )
            .child(div().size(board_size).flex_none().child(self.board.clone()))
            .child(
                h_flex()
                    .mt(px(14.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(tool("hint", "Hint", Self::hint, this.clone()))
                            .child(tool(
                                "show-answer",
                                "Show answer",
                                Self::show_answer,
                                this.clone(),
                            ))
                            .child(tool("why", "Why this move?", Self::why, this.clone())),
                    )
                    .child(tool("skip", "Skip", Self::skip, this.clone())),
            )
            .child(
                h_flex()
                    .mt(px(15.))
                    .gap(px(6.))
                    .child(recall(
                        "again",
                        ui::i18n::t("Again").to_owned(),
                        Grade::Again,
                        BtnKind::Normal,
                        this.clone(),
                    ))
                    .child(recall(
                        "hard",
                        format!("Hard ({}d)", intervals.hard),
                        Grade::Hard,
                        BtnKind::Normal,
                        this.clone(),
                    ))
                    .child(recall(
                        "good",
                        format!("Good ({}d)", intervals.good),
                        Grade::Good,
                        BtnKind::Primary,
                        this.clone(),
                    ))
                    .child(recall(
                        "easy",
                        format!("Easy ({}d)", intervals.easy),
                        Grade::Easy,
                        BtnKind::Normal,
                        this.clone(),
                    )),
            )
            .children(self.message.clone().map(|message| {
                div()
                    .mt(px(10.))
                    .text_size(px(13.))
                    .text_color(rgb(if self.phase == Phase::Wrong {
                        T.warn
                    } else {
                        T.fg_2
                    }))
                    .child(message)
            }))
    }

    fn render_leaks(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        const MIN_LEAK_GAMES: u32 = 2;
        const MIN_GAP_GAMES: u32 = 5;
        let this = cx.entity();
        let model = self.model.read(cx);
        let row = |title: String, subtitle: String, games: u32, warn: bool| {
            v_flex()
                .px(px(16.))
                .py(px(10.))
                .gap(px(2.))
                .border_t_1()
                .border_color(rgb(T.border))
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            mono(format!("{games} games"))
                                .text_size(px(14.))
                                .text_color(rgb(if warn { T.warn } else { T.muted })),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(subtitle),
                )
        };
        let mut body = v_flex();
        let mut shown = 0;
        for leak in model
            .leaks
            .iter()
            .filter(|leak| leak.games >= MIN_LEAK_GAMES)
            .take(2)
        {
            let number = leak.path.len() as u32;
            let played = chess_core::notation::numbered(number + 1, &leak.played);
            let title = match &leak.prepared {
                Some(prepared) => format!(
                    "{played}{} for {}",
                    if leak.prepared_score.is_some() {
                        "?!"
                    } else {
                        ""
                    },
                    chess_core::notation::numbered(number + 1, prepared)
                ),
                None => played,
            };
            let subtitle = match leak.prepared_score {
                Some(prepared) => format!(
                    "Score {:.0}% vs {:.0}% with the prepared move",
                    leak.score * 100.0,
                    prepared * 100.0
                ),
                None => {
                    let start = leak.path.len().saturating_sub(2);
                    format!("After {}", format_pv(start as u32, &leak.path[start..]))
                }
            };
            body = body.child(row(title, subtitle, leak.games, true));
            shown += 1;
        }
        for gap in model
            .gaps
            .iter()
            .filter(|gap| gap.games >= MIN_GAP_GAMES)
            .take(1)
        {
            let start = gap.path.len().saturating_sub(3);
            let title = format_pv(start as u32, &gap.path[start..]);
            body = body.child(row(
                title,
                ui::i18n::t("Not in repertoire yet").to_owned(),
                gap.games,
                false,
            ));
            shown += 1;
        }
        if shown == 0 {
            body = body.child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(
                        "No leaks found. Import your games to see where you leave the repertoire.",
                    ),
            );
        }
        panel()
            .child(
                h_flex().h(px(50.)).px(px(16.)).child(
                    h_flex()
                        .gap(px(10.))
                        .items_baseline()
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(ui::i18n::t("Leaks from your games")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted))
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(ui::i18n::t("Where you left the repertoire")),
                        ),
                ),
            )
            .child(body)
            .child(
                h_flex()
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(btn("drill-leaks", "Drill these lines").primary().on_click(
                        move |_, cx| {
                            this.update(cx, |screen, cx| screen.drill_leaks(cx));
                        },
                    )),
            )
    }
}

impl Render for TrainScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let board_size = ui::board::snap_board_size(
            (viewport.height - px(362.))
                .min(viewport.width - px(720.))
                .max(px(320.)),
            window.scale_factor(),
        );
        let mode_this = cx.entity();
        let mode = btn(
            "reply-mode",
            if self.every_reply {
                "Mode: every prepared reply"
            } else {
                "Mode: any prepared reply"
            },
        )
        .on_click(move |_, cx| {
            mode_this.update(cx, |this, cx| {
                this.every_reply = !this.every_reply;
                this.reload_queue(cx);
            })
        });
        let reset = cx.entity();
        let history = self.model.read(cx).repertoire.history.len();
        let session = v_flex().gap(px(6.)).mt(px(14.)).child(mode.full()).child(
            h_flex()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .child(format!("{history} completed reviews")),
                )
                .child(
                    btn("restart-study", "Start fresh")
                        .h(26.)
                        .pad(8.)
                        .text(12.)
                        .tooltip(ui::i18n::t(
                            "Discard this session and start the queue again",
                        ))
                        .on_click(move |_, cx| {
                            reset.update(cx, |this, cx| {
                                this.attempts.clear();
                                this.reload_queue(cx);
                            })
                        }),
                ),
        );
        let tree = self.render_tree(session, cx);
        let center = self.render_center(board_size, cx);
        let leaks = self.render_leaks(cx);
        h_flex()
            .items_start()
            .size_full()
            .child(tree)
            .child(
                div()
                    .id("train-center")
                    .px(px(28.))
                    .h_full()
                    .flex_none()
                    .overflow_y_scroll()
                    .child(center),
            )
            .child(
                div()
                    .id("leaks-column")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .pt(px(24.))
                    .pr(px(24.))
                    .pb(px(16.))
                    .child(leaks),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_lose_their_numbers() {
        assert_eq!(parse_line_text("1.e4 c5 2.Nf3"), vec!["e4", "c5", "Nf3"]);
        assert_eq!(parse_line_text("1. e4 1…c5"), vec!["e4", "c5"]);
        assert!(parse_line_text("  ").is_empty());
    }

    #[test]
    fn tree_lines_cover_each_leaf() {
        let tree = parse_pgn("1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *")
            .unwrap()
            .remove(0);
        let lines = tree_lines(&tree);
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&vec!["e4".to_owned(), "e5".to_owned(), "Nf3".to_owned()]));
        assert!(lines.contains(&vec!["e4".to_owned(), "c5".to_owned(), "Nf3".to_owned()]));
    }
}
