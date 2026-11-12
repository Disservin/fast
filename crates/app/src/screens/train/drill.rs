//! Drilling the repertoire: asking for a move, checking it, hints, answers and grading.

use super::*;

impl TrainScreen {
    pub(super) fn attempt_key(&self) -> String {
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

    pub(super) fn evidence(&mut self) -> &mut trainer::Attempt {
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

    pub(super) fn start_drill(&mut self, cx: &mut Context<Self>) {
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
        let (pos, last) = drill
            .as_ref()
            .and_then(|drill| Self::position_for(&drill.path))
            .unwrap_or_else(|| (Chess::default(), None));
        // The opponent's move that leads to the question sounds like a move on the board.
        if self.visible
            && let Some(mv) = last
        {
            self.model
                .read(cx)
                .config
                .board
                .play_move_sound(&mv, &pos, None);
        }
        let last = last.and_then(|mv| mv.from().map(|from| (from, mv.to())));
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, false, cx);
            board.set_arrows(Vec::new(), cx);
            board.set_player(Some(color), cx);
            board.set_flipped(!white, cx);
            board.set_interactive(drill.is_some(), cx);
        });
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn user_move(&mut self, mv: Move, cx: &mut Context<Self>) {
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
            let prompt = if self.evidence().scheduling_grade(Grade::Hard) == Grade::Again {
                "Correct: {}. Press Repeat to practise this position again."
            } else if self.evidence().hints > 0 {
                "Correct: {}. Hint used: choose Repeat or Hard."
            } else {
                "Correct: {}. How well did you know it?"
            };
            self.message = Some(ui::i18n::tf(prompt, &[&played]).into());
            let last = mv.from().map(|from| (from, mv.to()));
            self.model
                .read(cx)
                .config
                .board
                .play_move_sound(&mv, &after, None);
            self.board.update(cx, |board, cx| {
                board.set_position(after, last, true, cx);
                board.set_interactive(false, cx);
            });
        } else {
            if !prepared {
                self.evidence().failures += 1;
            }
            self.phase = Phase::Wrong;
            self.model.read(cx).config.board.play_wrong_sound();
            self.message = Some(if prepared {
                format!("{played} is a valid repertoire move. This exercise asks for another prepared reply. Try again.").into()
            } else {
                format!("{played} is not in your repertoire here. Try again.").into()
            });
        }
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn answer_move(&self) -> Option<Move> {
        let drill = self.current()?;
        let (pos, _) = Self::position_for(&drill.path)?;
        let parsed: chess_core::shakmaty::san::San = drill.answer.parse().ok()?;
        parsed.to_move(&pos).ok()
    }

    pub(super) fn hint(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Asking | Phase::Wrong) {
            return;
        }
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

    pub(super) fn show_answer(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Asking | Phase::Wrong) {
            return;
        }
        let Some(mv) = self.answer_move() else { return };
        let Some(from) = mv.from() else { return };
        self.evidence().reveals += 1;
        self.phase = Phase::Revealed;
        self.message = Some(ui::i18n::t("Answer revealed. This position will repeat.").into());
        self.board.update(cx, |board, cx| {
            board.set_arrows(vec![Arrow::new(from, mv.to())], cx);
            board.set_interactive(false, cx);
        });
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn why(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn skip(&mut self, cx: &mut Context<Self>) {
        self.evidence();
        if self.queue.len() > 1 {
            let drill = self.queue.remove(0);
            self.queue.push(drill);
        }
        self.start_drill(cx);
    }

    pub(super) fn grade(&mut self, grade: Grade, cx: &mut Context<Self>) {
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

    /// The drill controls beside the board: what the trainer says about the last
    /// answer, Hint, Show answer and Skip while the move is asked, and the recall
    /// grades once it is answered. Shown only while there is a position to drill or
    /// something to say.
    pub(super) fn render_drill(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let this = cx.entity();
        let drill = self.current().cloned();
        if drill.is_none() && self.message.is_none() {
            return None;
        }
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
        let answered = has_drill && matches!(self.phase, Phase::Correct | Phase::Revealed);
        let evidence = self
            .attempts
            .get(&self.attempt_key())
            .cloned()
            .unwrap_or_default();
        let repeat = evidence.scheduling_grade(Grade::Hard) == Grade::Again;
        let hinted = evidence.hints > 0;
        let explanation = if repeat {
            Some(ui::i18n::t(
                "A mistake or reveal means this position will repeat.",
            ))
        } else if hinted {
            Some(ui::i18n::t(
                "Hint used. The highest available grade is Hard.",
            ))
        } else {
            None
        };
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
                    .disabled(!answered || evidence.scheduling_grade(grade) != grade)
                    .on_click(move |_, cx| this.update(cx, |screen, cx| screen.grade(grade, cx)))
            };
        let grades = [
            (
                "again",
                ui::i18n::t("Repeat").to_owned(),
                Grade::Again,
                BtnKind::Normal,
                true,
            ),
            (
                "hard",
                format!("{} ({}d)", ui::i18n::t("Hard"), intervals.hard),
                Grade::Hard,
                BtnKind::Normal,
                !repeat,
            ),
            (
                "good",
                format!("{} ({}d)", ui::i18n::t("Good"), intervals.good),
                Grade::Good,
                BtnKind::Primary,
                !repeat && !hinted,
            ),
            (
                "easy",
                format!("{} ({}d)", ui::i18n::t("Easy"), intervals.easy),
                Grade::Easy,
                BtnKind::Normal,
                !repeat && !hinted,
            ),
        ];
        let mut grade_rows = v_flex().gap(px(6.));
        for pair in grades.chunks(2) {
            let mut row = h_flex().gap(px(6.));
            for (id, label, grade, kind, shown) in pair.iter().cloned() {
                if shown {
                    row = row.child(recall(id, label, grade, kind, this.clone()));
                }
            }
            grade_rows = grade_rows.child(row);
        }
        Some(
            panel()
                .flex_none()
                .child(
                    ui::widgets::panel_header("Drill").when(has_drill && !answered, |header| {
                        header.child(
                            tool("skip", "Skip", Self::skip, this.clone())
                                .compact()
                                .tooltip(ui::i18n::t("Ask this position again later")),
                        )
                    }),
                )
                .child(
                    v_flex()
                        .px(px(16.))
                        .py(px(14.))
                        .gap(px(10.))
                        .children(self.render_message())
                        .when(has_drill && !answered, |column| {
                            column.child(
                                h_flex()
                                    .gap(px(6.))
                                    .child(tool("hint", "Hint", Self::hint, this.clone()).flex_1())
                                    .child(
                                        tool(
                                            "show-answer",
                                            "Show answer",
                                            Self::show_answer,
                                            this.clone(),
                                        )
                                        .flex_1(),
                                    ),
                            )
                        })
                        .when(answered, |column| {
                            column
                                .children(explanation.map(|text| {
                                    div().text_size(px(13.)).text_color(rgb(T.fg_2)).child(text)
                                }))
                                .child(grade_rows)
                                .child(
                                    tool("why", "Why this move?", Self::why, this.clone())
                                        .compact(),
                                )
                        }),
                ),
        )
    }

    /// What the trainer says about the last answer, in a box at the top of the Drill panel: a
    /// wrong move gets a `warn` outline and a cross, a right one `good` and a tick. The
    /// text itself stays in `fg` so it reads well in every palette.
    pub(super) fn render_message(&self) -> Option<impl IntoElement + use<>> {
        let message = self.message.clone()?;
        let (outline, mark) = match self.phase {
            Phase::Wrong => (T.warn, Some("icons/x.svg")),
            Phase::Correct => (T.good, Some("icons/check.svg")),
            Phase::Asking | Phase::Revealed => (T.border_strong, None),
        };
        Some(
            h_flex()
                .gap(px(8.))
                .px(px(12.))
                .py(px(9.))
                .rounded(px(6.))
                .bg(rgb(T.surface))
                .border_1()
                .border_color(rgb(outline))
                .children(mark.map(|path| ui::widgets::icon(path, px(16.), outline)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(T.fg))
                        .child(message),
                ),
        )
    }
}
