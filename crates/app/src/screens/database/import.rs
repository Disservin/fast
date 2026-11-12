//! Importing PGN files into a database, and exporting games.

use super::*;

/// The toast after an import: games, how many have annotations, skipped games and
/// variations left out.
pub(super) fn import_message(summary: &db::ImportSummary) -> String {
    let mut message = ui::i18n::tf(
        "Imported {} games, {} with annotations.",
        &[
            &format_count(summary.games),
            &format_count(summary.annotated),
        ],
    );
    if summary.skipped > 0 {
        message.push(' ');
        message.push_str(&ui::i18n::tf(
            "{} games skipped.",
            &[&format_count(summary.skipped)],
        ));
    }
    if summary.variations_dropped > 0 {
        message.push(' ');
        message.push_str(&ui::i18n::tf(
            "{} variations had illegal moves and were left out.",
            &[&format_count(summary.variations_dropped)],
        ));
    }
    message
}

impl DatabaseScreen {
    /// Opens the Import dialog, unless an import is already running.
    pub fn prompt_import(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.importing.is_some() {
            return;
        }
        self.import_dialog = true;
        cx.notify();
    }

    /// Closes the Import dialog and asks for the PGN file to import.
    pub(super) fn choose_import_file(&mut self, cx: &mut Context<Self>) {
        self.import_dialog = false;
        cx.notify();
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Import PGN",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |this, cx| this.import_file(path, false, cx));
            },
        );
    }

    /// Empties a database and imports its source file again, so games imported before
    /// comments and variations were kept get them.
    pub(super) fn reimport_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some((database, file)) = self.confirm_reimport.take() else {
            return;
        };
        if self.importing.is_some() {
            return;
        }
        self.selected_db = Some(database);
        self.selected = None;
        self.import_file(file, true, cx);
    }

    /// Streams a PGN into the selected database, with progress and cancel. With
    /// `replace`, the database is emptied first.
    pub fn import_file(&mut self, file: PathBuf, replace: bool, cx: &mut Context<Self>) {
        let target = self
            .selected_db
            .clone()
            .unwrap_or_else(|| self.model.read(cx).my_games_path());
        let control = ImportControl::default();
        let options = ImportOptions {
            keep_annotations: self.model.read(cx).config.import_keep_annotations,
            build_structure_index: self.model.read(cx).config.import_structure_index,
            ..Default::default()
        };
        self.importing = Some(Importing {
            control: control.clone(),
            progress: ImportProgress::default(),
            file: file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        });
        let (progress_tx, progress_rx) = smol::channel::bounded(1);
        let worker = cx.background_spawn({
            let target = target.clone();
            async move {
                let reader = std::fs::File::open(&file)?;
                if replace {
                    Database::open(&target)?.clear_games()?;
                }
                let summary = import_games(&target, reader, &options, &control, |progress| {
                    let _ = progress_tx.try_send(progress);
                })?;
                if let Ok(database) = Database::open(&target) {
                    if target.file_stem().is_some_and(|stem| stem == "my_games") {
                        let _ = database.set_name("My games");
                    }
                    let _ = database.set_source_file(&file);
                }
                anyhow::Ok(summary)
            }
        });
        cx.spawn(async move |this, cx| {
            while let Ok(progress) = progress_rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(importing) = &mut this.importing {
                        importing.progress = progress;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let outcome = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = None;
                let message = match &outcome {
                    Ok(summary) if summary.cancelled => ui::i18n::tf(
                        "Import cancelled after {} games.",
                        &[&format_count(summary.games)],
                    ),
                    Ok(summary) => import_message(summary),
                    Err(err) => format!("{} {err:#}", ui::i18n::t("Import failed:")),
                };
                let dropped = outcome
                    .as_ref()
                    .ok()
                    .map(|summary| summary.games_with_dropped.clone())
                    .filter(|games| !games.is_empty());
                this.selected_db = Some(target);
                let screen = cx.entity().downgrade();
                this.model.update(cx, |model, cx| {
                    match dropped {
                        Some(games) => model.toast_with_action(
                            message,
                            "Show details",
                            move |_, cx| {
                                let games = games.clone();
                                let _ = screen.update(cx, |screen, cx| {
                                    screen.dropped_details = Some(games);
                                    cx.notify();
                                });
                            },
                            cx,
                        ),
                        None => model.toast(message, cx),
                    }
                    model.refresh_databases();
                    model.refresh_leaks(cx);
                    cx.emit(ModelEvent::Databases);
                });
            });
        })
        .detach();
        cx.notify();
    }

    /// Saves matching games as PGN after choosing a folder and filename in Save As.
    pub(super) fn export_pgn(&mut self, cx: &mut Context<Self>) {
        let Some(db_path) = self.selected_db.clone() else {
            return;
        };
        let ids = self.ids.clone();
        let this = cx.entity().downgrade();
        let model = self.model.clone();
        let name = self
            .db_snapshot
            .as_ref()
            .map_or("database", |entry| entry.name.as_str());
        crate::model::save_pgn(model, name, cx, move |path, cx| {
            let Some(this) = this.upgrade() else { return };
            this.update(cx, |_, cx| {
                cx.spawn(async move |this, cx| {
                    let written = cx
                        .background_spawn(async move {
                            use std::io::Write as _;
                            let database = Database::open(db_path)?;
                            let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
                            let mut count = 0usize;
                            for id in ids {
                                if let Some(game) = database.game(id)? {
                                    writeln!(out, "{}", game.to_pgn())?;
                                    count += 1;
                                }
                            }
                            out.flush()?;
                            anyhow::Ok(count)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        let text = match written {
                            Ok(count) => format!("Exported {count} games."),
                            Err(err) => format!("Export failed: {err:#}"),
                        };
                        this.model.toast(text, cx);
                    });
                })
                .detach();
            });
        });
    }

    /// The Import dialog: a PGN file or a player's games from Lichess or Chess.com,
    /// where they go, and while a download runs, its progress.
    pub(super) fn render_import_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.import_dialog {
            return None;
        }
        let this = cx.entity();
        let keep = self.model.read(cx).config.import_keep_annotations;
        let selected_name = self.db_snapshot.as_ref().map(|entry| entry.name.clone());
        let source_this = this.clone();
        let source = segmented(
            "import-source",
            vec![
                (ImportSource::File, ui::i18n::t("PGN file").into(), false),
                (
                    ImportSource::Site(Site::Lichess),
                    Site::Lichess.name().into(),
                    false,
                ),
                (
                    ImportSource::Site(Site::ChessCom),
                    Site::ChessCom.name().into(),
                    false,
                ),
            ],
            self.import_source,
            move |value, _, cx| {
                source_this.update(cx, |screen, cx| {
                    screen.import_source = value;
                    screen.download_error = None;
                    cx.notify();
                })
            },
        );
        let label = |text: &str| {
            div()
                .w(px(72.))
                .flex_none()
                .text_size(px(13.))
                .text_color(rgb(T.muted))
                .child(ui::i18n::t(text).to_owned())
        };
        let model = self.model.clone();
        let keep_check = check(
            "import-keep-annotations",
            keep,
            "Keep comments and variations",
        )
        .on_change(move |value, _, cx| {
            model.update(cx, |model, cx| {
                model.config.import_keep_annotations = value;
                model.save_config(cx);
            });
        });
        let mut body = v_flex()
            .px(px(16.))
            .py(px(14.))
            .gap(px(10.))
            .child(h_flex().gap(px(8.)).child(label("Source")).child(source));
        let footer = h_flex()
            .justify_end()
            .gap(px(6.))
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(rgb(T.border));
        let footer = match self.import_source {
            ImportSource::File => {
                let target = selected_name
                    .clone()
                    .unwrap_or_else(|| SharedString::from(ui::i18n::t("My games")));
                body = body
                    .child(
                        h_flex().gap(px(8.)).child(label("Into")).child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(target),
                        ),
                    )
                    .child(keep_check)
                    .child({
                        let model = self.model.clone();
                        check(
                            "import-structure-index",
                            model.read(cx).config.import_structure_index,
                            "Build structure index",
                        )
                        .on_change(move |value, _, cx| {
                            model.update(cx, |model, cx| {
                                model.config.import_structure_index = value;
                                model.save_config(cx);
                            })
                        })
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(ui::i18n::t(
                                "Turn this off for very large reference files where size matters.",
                            )),
                    );
                let (cancel, choose) = (this.clone(), this.clone());
                footer
                    .child(btn("import-cancel", "Cancel").on_click(move |_, cx| {
                        cancel.update(cx, |screen, cx| {
                            screen.import_dialog = false;
                            cx.notify();
                        })
                    }))
                    .child(
                        btn("import-choose", "Choose PGN file…")
                            .primary()
                            .disabled(self.downloading.is_some())
                            .on_click(move |_, cx| {
                                choose.update(cx, |screen, cx| screen.choose_import_file(cx))
                            }),
                    )
            }
            ImportSource::Site(site) => {
                if let Some(running) = &self.downloading {
                    body = body.child(self.render_download_progress(running));
                } else {
                    let has_account = self
                        .typed_account(site, cx)
                        .is_some_and(|account| account.last_game_ms.is_some());
                    let since_this = this.clone();
                    let since = segmented(
                        "download-since",
                        vec![
                            (Since::All, ui::i18n::t("All").into(), false),
                            (Since::LastYear, ui::i18n::t("Last year").into(), false),
                            (Since::LastMonth, ui::i18n::t("Last month").into(), false),
                            (
                                Since::LastUpdate,
                                ui::i18n::t("Since last update").into(),
                                !has_account,
                            ),
                        ],
                        if self.download_since == Since::LastUpdate && !has_account {
                            Since::All
                        } else {
                            self.download_since
                        },
                        move |value, _, cx| {
                            since_this.update(cx, |screen, cx| {
                                screen.download_since = value;
                                cx.notify();
                            })
                        },
                    );
                    let into_this = this.clone();
                    let my_games = SharedString::from(ui::i18n::t("My games"));
                    let into = match selected_name.clone().filter(|name| *name != my_games) {
                        Some(name) => segmented(
                            "download-into",
                            vec![(false, my_games, false), (true, name, false)],
                            self.download_into_selected,
                            move |value, _, cx| {
                                into_this.update(cx, |screen, cx| {
                                    screen.download_into_selected = value;
                                    cx.notify();
                                })
                            },
                        ),
                        None => h_flex().child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(my_games),
                        ),
                    };
                    let (clocks_this, me_this) = (this.clone(), this.clone());
                    body = body
                        .child(
                            h_flex().gap(px(8.)).child(label("Username")).child(
                                div()
                                    .w(px(220.))
                                    .child(text_input(&self.download_user).height(32.)),
                            ),
                        )
                        .child(h_flex().gap(px(8.)).child(label("Since")).child(since))
                        .child(h_flex().gap(px(8.)).child(label("Into")).child(into))
                        .when(site == Site::Lichess, |body| {
                            body.child(
                                check(
                                    "download-clocks",
                                    self.download_clocks,
                                    "Include clocks and evaluations",
                                )
                                .on_change(move |value, _, cx| {
                                    clocks_this.update(cx, |screen, cx| {
                                        screen.download_clocks = value;
                                        cx.notify();
                                    })
                                }),
                            )
                        })
                        .child(
                            check("download-is-me", self.download_is_me, "This is my account")
                                .on_change(move |value, _, cx| {
                                    me_this.update(cx, |screen, cx| {
                                        screen.download_is_me = value;
                                        cx.notify();
                                    })
                                }),
                        )
                        .child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                            ui::i18n::tf(
                                "Fast connects to {} to fetch the games. Nothing else is sent.",
                                &[site.host()],
                            ),
                        ));
                }
                if let Some(error) = &self.download_error {
                    body = body.child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.warn))
                            .child(error.clone()),
                    );
                }
                let (close, act) = (this.clone(), this.clone());
                let running = self.downloading.is_some();
                let close_button = btn("download-close", if running { "Close" } else { "Cancel" });
                let close_button = if running {
                    close_button.tooltip("The download keeps running")
                } else {
                    close_button
                };
                footer
                    .child(close_button.on_click(move |_, cx| {
                        close.update(cx, |screen, cx| {
                            screen.import_dialog = false;
                            cx.notify();
                        })
                    }))
                    .child(if running {
                        btn("download-stop", "Cancel download")
                            .disabled(
                                self.downloading
                                    .as_ref()
                                    .is_some_and(|running| running.control.is_cancelled()),
                            )
                            .on_click(move |_, cx| {
                                act.update(cx, |screen, cx| {
                                    if let Some(running) = &screen.downloading {
                                        running.control.cancel();
                                    }
                                    cx.notify();
                                })
                            })
                    } else {
                        btn("download-start", "Download games")
                            .primary()
                            .disabled(self.importing.is_some())
                            .on_click(move |_, cx| {
                                act.update(cx, |screen, cx| screen.download_from_dialog(cx))
                            })
                    })
            }
        };
        Some(dialog(
            "import-dialog",
            "Import games",
            480.,
            v_flex().child(body).child(footer).into_any_element(),
            this,
        ))
    }

    /// Asks before emptying a database to import its source file again.
    pub(super) fn render_confirm_reimport(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (database, file) = self.confirm_reimport.as_ref()?;
        let this = cx.entity();
        let name = self
            .model
            .read(cx)
            .databases
            .iter()
            .find(|entry| &entry.path == database)
            .map_or_else(|| SharedString::from(""), |entry| entry.name.clone());
        let (cancel, confirm) = (this.clone(), this.clone());
        let body = v_flex()
            .child(
                v_flex()
                    .px(px(16.))
                    .py(px(14.))
                    .gap(px(4.))
                    .child(div().text_size(px(14.)).child(ui::i18n::tf(
                        "Replace the games in {} with {}?",
                        &[&name, &file.display().to_string()],
                    )))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(ui::i18n::t(
                                "Edits and reviews of these games are lost. This cannot be undone.",
                            )),
                    ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(6.))
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(btn("reimport-cancel", "Cancel").on_click(move |_, cx| {
                        cancel.update(cx, |screen, cx| {
                            screen.confirm_reimport = None;
                            cx.notify();
                        })
                    }))
                    .child(btn("reimport-confirm", "Re-import").primary().on_click(
                        move |_, cx| confirm.update(cx, |screen, cx| screen.reimport_confirmed(cx)),
                    )),
            );
        Some(dialog(
            "confirm-reimport",
            "Re-import from file",
            460.,
            body.into_any_element(),
            this,
        ))
    }

    /// The games whose variations had illegal moves in the last import, by their
    /// number in the file.
    pub(super) fn render_dropped_details(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let games = self.dropped_details.as_ref()?;
        let this = cx.entity();
        let shown: Vec<String> = games.iter().take(200).map(|n| format_count(*n)).collect();
        let more = games.len().saturating_sub(shown.len());
        let body = v_flex()
            .px(px(16.))
            .py(px(14.))
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(
                        "These games kept their moves, but a variation with an illegal move was left out. Numbers count games in the file from 1.",
                    )),
            )
            .child(
                mono(shown.join(", "))
                    .text_size(px(13.))
                    .text_color(rgb(T.fg_2)),
            )
            .when(more > 0, |body| {
                body.child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::tf("and {} more", &[&format_count(more as u64)])),
                )
            });
        Some(dialog(
            "dropped-details",
            "Variations left out",
            460.,
            body.into_any_element(),
            this,
        ))
    }

    pub(super) fn render_import_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let importing = self.importing.as_ref()?;
        let this = cx.entity();
        Some(
            panel()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .justify_between()
                .px(px(16.))
                .py(px(8.))
                .child(div().text_size(px(14.)).child(format!(
                    "Importing {}: {} games",
                    importing.file,
                    format_count(importing.progress.games)
                )))
                .child(btn("cancel-import", ui::i18n::t("Cancel")).h(30.).on_click(
                    move |_, cx| {
                        this.update(cx, |screen, _| {
                            if let Some(importing) = &screen.importing {
                                importing.control.cancel();
                            }
                        });
                    },
                )),
        )
    }
}
