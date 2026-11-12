//! The New tournament dialog: choosing a form, pasted config or saved run, and starting it.

use super::*;

impl TournamentScreen {
    pub(super) fn participants(&self, cx: &Context<Self>) -> Vec<Participant> {
        let model = self.model.read(cx);
        self.selected
            .iter()
            .filter_map(|id| model.engine(*id))
            .map(|engine| Participant {
                name: engine.name.clone(),
                cmd: engine.path.clone(),
                options: engine.options.clone(),
            })
            .chain(self.extra_engines.iter().cloned())
            .collect()
    }

    pub(super) fn setup(&self, cx: &Context<Self>) -> Setup {
        Setup {
            participants: self.participants(cx),
            limit: self.limit,
            limit_value: match self.limit {
                Limit::TimeControl => self
                    .clock_input
                    .milliseconds(cx)
                    .map(|(base, inc)| {
                        format!(
                            "{}+{}",
                            crate::controls::time_seconds(base),
                            crate::controls::time_seconds(inc)
                        )
                    })
                    .unwrap_or_default(),
                Limit::MoveTime => self
                    .move_time
                    .milliseconds(cx)
                    .filter(|ms| *ms > 0)
                    .map(crate::controls::time_seconds)
                    .unwrap_or_default(),
                _ => self.text(&self.limit_value, cx),
            },
            hash: self.text(&self.hash, cx),
            threads: self.text(&self.threads, cx),
            rounds: self.text(&self.rounds, cx),
            games_per_round: self.games_per_round,
            concurrency: self.text(&self.concurrency, cx),
            gauntlet: self.gauntlet,
            chess960: self.chess960,
            noswap: self.noswap,
            reverse: self.reverse,
            openings: self.text(&self.openings, cx),
            random_order: self.random_order,
            plies: self.text(&self.plies, cx),
            sprt: self.sprt,
            elo0: self.text(&self.elo0, cx),
            elo1: self.text(&self.elo1, cx),
            alpha: self.text(&self.alpha, cx),
            beta: self.text(&self.beta, cx),
            sprt_model: self.sprt_model,
            draw_adjudication: self.draw_adjudication,
            draw_movenumber: self.text(&self.draw_movenumber, cx),
            draw_movecount: self.text(&self.draw_movecount, cx),
            draw_score: self.text(&self.draw_score, cx),
            resign_adjudication: self.resign_adjudication,
            resign_movecount: self.text(&self.resign_movecount, cx),
            resign_score: self.text(&self.resign_score, cx),
            resign_twosided: self.resign_twosided,
            maxmoves: self.text(&self.maxmoves, cx),
            tb_paths: if self.tb_adjudication {
                self.text(&self.tb_paths, cx)
            } else {
                String::new()
            },
            tb_pieces: self.text(&self.tb_pieces, cx),
            tb_ignore50: self.tb_ignore50,
            tb_adjudicate: self.tb_adjudicate,
            recover: self.recover,
            seed: self.text(&self.seed, cx),
            wait: self.text(&self.wait, cx),
            time_margin: self.text(&self.time_margin, cx),
            restart: self.restart,
            affinity: self.affinity,
            engine_args: self.text(&self.engine_args, cx),
            pgn: PgnOut {
                match_line: self.text(&self.pgn_match_line, cx),
                ..self.pgn.clone()
            },
            extra: self.text(&self.extra, cx),
        }
    }

    /// The executable to run: the user's own, or the pinned download if present.
    pub(super) fn program(&self, cx: &Context<Self>) -> Option<PathBuf> {
        let custom = self.text(&self.fastchess_path, cx);
        if !custom.is_empty() {
            return Some(PathBuf::from(custom));
        }
        fastchess::installed(&self.data_dir(cx))
    }

    pub(super) fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.form_error = Some(message.into());
        cx.notify();
    }

    pub(super) fn start_form(&mut self, cx: &mut Context<Self>) {
        if self.book_task.is_some() {
            return;
        }
        let mut setup = self.setup(cx);
        let index = self
            .book
            .read(cx)
            .selected_index(cx)
            .map(|index| index.row)
            .unwrap_or(0);
        if let Some(&name) = crate::opening_books::BOOKS.get(index) {
            let data = self.data_dir(cx);
            let path = crate::opening_books::path(&data, name);
            setup.openings = path.to_string_lossy().into_owned();
            if !path.is_file() {
                self.form_error = Some(format!("Downloading {name}…").into());
                let worker =
                    cx.background_spawn(async move { crate::opening_books::install(&data, name) });
                self.book_task = Some(cx.spawn(async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update(cx, |screen, cx| {
                        screen.book_task = None;
                        match result {
                            Ok(()) => {
                                screen.form_error = None;
                                screen.start_setup(setup, cx);
                            }
                            Err(err) => screen.fail(
                                format!("Book download failed: {err:#}. Start again to retry."),
                                cx,
                            ),
                        }
                    });
                }));
                cx.notify();
                return;
            }
        } else if index == crate::opening_books::BOOKS.len() {
            setup.openings.clear();
        }
        self.start_setup(setup, cx);
    }

    pub(super) fn start_setup(&mut self, setup: Setup, cx: &mut Context<Self>) {
        let dir = match fastchess::new_run_dir(&self.data_dir(cx)) {
            Ok(dir) => dir,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        match setup.args(&dir) {
            Ok(args) => self.launch(
                Launch {
                    cwd: dir.clone(),
                    config: dir.join("config.json"),
                    dir,
                    args,
                },
                cx,
            ),
            Err(err) => {
                let _ = std::fs::remove_dir(&dir);
                self.fail(format!("{err:#}"), cx)
            }
        }
    }

    pub(super) fn start_paste(&mut self, cx: &mut Context<Self>) {
        let text = self.paste.read(cx).value().to_string();
        let pasted = match fastchess::parse_paste(&text) {
            Ok(pasted) => pasted,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let dir = match fastchess::new_run_dir(&self.data_dir(cx)) {
            Ok(dir) => dir,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let custom_cwd = self.text(&self.paste_dir, cx);
        let cwd = if custom_cwd.is_empty() || matches!(pasted, Pasted::Config(_)) {
            dir.clone()
        } else {
            PathBuf::from(custom_cwd)
        };
        if !cwd.is_dir() {
            let _ = std::fs::remove_dir(&dir);
            return self.fail(format!("Folder not found: {}", cwd.display()), cx);
        }
        match pasted.launch(&dir, &cwd) {
            Ok((args, config)) => self.launch(
                Launch {
                    dir,
                    cwd,
                    config,
                    args,
                },
                cx,
            ),
            Err(err) => self.fail(format!("{err:#}"), cx),
        }
    }

    pub(super) fn render_paste(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let load = cx.entity();
        v_flex()
            .gap(px(10.))
            .child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                "A command line starts a new tournament; a config.json continues one, \
                         including its results so far.",
            ))
            .child(
                div()
                    .rounded(px(4.))
                    .bg(rgb(T.bg))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .font_family(MONO)
                    .text_size(px(12.5))
                    .child(Textarea::new(&self.paste).appearance(false).h(px(300.))),
            )
            .child(btn("tournament-load-config", "Load config file…").on_click(
                move |window, cx| load.update(cx, |screen, cx| screen.load_config_file(window, cx)),
            ))
            .child(field(
                "Run a command line in folder (for its relative paths)",
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().child(text_input(&self.paste_dir)))
                    .child(crate::controls::browse_path(
                        "tournament-run-folder",
                        &self.model,
                        &self.paste_dir,
                        "Run a command line in folder (for its relative paths)",
                        true,
                    )),
            ))
    }

    pub(super) fn render_saved(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let busy = self.run.as_ref().is_some_and(Run::active);
        let current = self.run.as_ref().map(|run| run.dir.clone());
        let rows: Vec<_> = self
            .saved
            .iter()
            .filter(|saved| Some(&saved.dir) != current.as_ref())
            .take(8)
            .cloned()
            .collect();
        if rows.is_empty() {
            return None;
        }
        let mut list = v_flex()
            .gap(px(6.))
            .child(section_label("Earlier tournaments"));
        for (index, saved) in rows.into_iter().enumerate() {
            let this = cx.entity();
            let (dir, config) = (saved.dir.clone(), saved.config.clone());
            list = list.child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .overflow_hidden()
                                    .child(SharedString::from(saved.engines.join(", "))),
                            )
                            .child(
                                h_flex()
                                    .gap(px(12.))
                                    .text_size(px(12.))
                                    .text_color(rgb(T.muted))
                                    .child(SharedString::from(format!(
                                        "{} games saved",
                                        saved.games
                                    )))
                                    .child(
                                        div().font_family(ui::MONO).text_color(rgb(T.faint)).child(
                                            SharedString::from(
                                                saved
                                                    .dir
                                                    .file_name()
                                                    .map(|name| name.to_string_lossy().into_owned())
                                                    .unwrap_or_default(),
                                            ),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        btn(("tournament-continue", index), "Continue")
                            .disabled(busy)
                            .on_click(move |_, cx| {
                                let (dir, config) = (dir.clone(), config.clone());
                                let cwd = dir.clone();
                                this.update(cx, |screen, cx| screen.resume(dir, cwd, config, cx))
                            }),
                    ),
            );
        }
        Some(list)
    }

    pub(super) fn open_setup(&mut self, cx: &mut Context<Self>) {
        self.setup_open = true;
        self.form_error = None;
        cx.notify();
    }

    pub(super) fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.setup_open = false;
        cx.notify();
    }

    /// The tournament setup, over the tab. Closes once fastchess starts.
    pub(super) fn render_setup(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let source_this = cx.entity();
        let start = cx.entity();
        let close = cx.entity();
        let busy = self.run.as_ref().is_some_and(Run::active) || self.pending.is_some();
        let source = self.source;
        let body = match source {
            Source::Form => self.render_form(window, cx).into_any_element(),
            Source::Paste => self.render_paste(cx).into_any_element(),
        };
        let dialog_width = (f32::from(window.viewport_size().width) * 0.7).clamp(760., 1200.);
        let body = v_flex()
            .flex_none()
            .gap(px(16.))
            .px(px(16.))
            .pt(px(14.))
            .pb(px(16.))
            .child(self.render_fastchess(cx))
            .child(segmented(
                "tournament-source",
                vec![
                    (Source::Form, "Set up".into(), false),
                    (Source::Paste, "Paste config".into(), false),
                ],
                source,
                move |source, _, cx| {
                    source_this.update(cx, |screen, cx| {
                        screen.source = source;
                        screen.form_error = None;
                        cx.notify();
                    })
                },
            ))
            .child(body)
            .children(self.form_error.clone().map(|error| {
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.warn))
                    .child(error)
            }))
            .child(
                btn("tournament-start", "Start tournament")
                    .primary()
                    .full()
                    .h(38.)
                    .disabled(busy)
                    .on_click(move |_, cx| {
                        start.update(cx, |screen, cx| match screen.source {
                            Source::Form => screen.start_form(cx),
                            Source::Paste => screen.start_paste(cx),
                        })
                    }),
            );
        ui::widgets::dialog(
            "tournament-setup",
            "New tournament",
            dialog_width,
            body,
            move |cx| close.update(cx, |screen, cx| screen.close_setup(cx)),
        )
        .into_any_element()
    }
}
