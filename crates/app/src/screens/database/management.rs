//! Adding collections and editing the selected database's details and contents.

use super::*;

/// The collection details being edited independently of table filters.
pub(super) struct Manager {
    path: PathBuf,
    name: Entity<InputState>,
    description: Entity<InputState>,
    from: Entity<InputState>,
    into: Entity<InputState>,
    reference: bool,
    stats: Option<db::DatabaseStats>,
    bytes: u64,
    busy: bool,
    error: Option<String>,
}

/// A local file or web PGN to turn into a new independent collection.
pub(super) struct AddDatabase {
    web: bool,
    name: Entity<InputState>,
    description: Entity<InputState>,
    source: Entity<InputState>,
    web_source: Entity<InputState>,
}

/// The maintenance operation explicitly requested in the management dialog.
#[derive(Clone, Copy)]
enum Action {
    Save,
    Merge,
    Duplicates,
    Empty,
}

impl DatabaseScreen {
    /// Prevents collection maintenance while another writer on this screen is running.
    pub(super) fn collection_busy(&self) -> bool {
        self.importing.is_some()
            || self.downloading.is_some()
            || self.reviewing.is_some()
            || self.pattern_control.is_some()
            || self.collection_control.is_some()
            || self.manager.as_ref().is_some_and(|manager| manager.busy)
    }

    /// Opens the add dialog without starting any network traffic.
    pub(super) fn open_add_database(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collection_error = None;
        self.add_database = Some(AddDatabase {
            web: false,
            name: new_input("", "Database name", window, cx),
            description: new_input("", "Description", window, cx),
            web_source: new_input("", "HTTPS download URL", window, cx),
            source: new_input("", "PGN, PGN.gz, ZIP or Fast SQLite file", window, cx),
        });
        cx.notify();
    }

    /// Chooses a local source while keeping the new collection's details visible.
    fn browse_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let handle = window.window_handle();
        pick_path(
            self.model.clone(),
            "Add database",
            None,
            cx,
            move |path, cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |screen, cx| {
                        if let Some(add) = &screen.add_database {
                            add.source.update(cx, |input, cx| {
                                input.set_value(path.to_string_lossy().into_owned(), window, cx)
                            });
                        }
                        cx.notify();
                    });
                });
            },
        );
    }

    /// Starts a cancellable background import or download into a new database file.
    fn install_collection(&mut self, twic: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.collection_busy() {
            return;
        }
        let Some(add) = &self.add_database else {
            return;
        };
        let name = if twic {
            "The Week in Chess".to_owned()
        } else {
            add.name.read(cx).value().trim().to_owned()
        };
        let description = add.description.read(cx).value().trim().to_owned();
        let input = if add.web {
            &add.web_source
        } else {
            &add.source
        }
        .read(cx)
        .value()
        .trim()
        .to_owned();
        if name.is_empty() || (!twic && input.is_empty()) {
            self.collection_error = Some(ui::i18n::t("Enter a name and a source.").to_owned());
            cx.notify();
            return;
        }
        let source = if twic {
            crate::database_sources::Source::Twic
        } else if add.web {
            crate::database_sources::Source::Web(input)
        } else {
            crate::database_sources::Source::Local(PathBuf::from(input))
        };
        let model = self.model.read(cx);
        let directory = model.paths.databases_dir();
        let options = ImportOptions {
            keep_annotations: model.config.import_keep_annotations,
            build_structure_index: model.config.import_structure_index,
            ..Default::default()
        };
        let control = ImportControl::default();
        self.collection_control = Some(control.clone());
        self.collection_progress = ui::i18n::t("Preparing database…").to_owned();
        self.collection_error = None;
        let (send, receive) = smol::channel::bounded(1);
        let worker = cx.background_spawn(async move {
            crate::database_sources::install(
                &directory,
                source,
                &name,
                &description,
                &options,
                &control,
                |progress| {
                    let _ = send.try_send(progress);
                },
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(progress) = receive.recv().await {
                let _ = this.update(cx, |screen, cx| {
                    screen.collection_progress = progress;
                    cx.notify();
                });
            }
            let result = worker.await;
            let _ = this.update_in(cx, |screen, window, cx| {
                screen.collection_control = None;
                match result {
                    Ok(path) => {
                        screen.add_database = None;
                        screen.selected_db = Some(path);
                        screen.selected = None;
                        screen.model.update(cx, |model, cx| {
                            model.refresh_databases();
                            cx.emit(ModelEvent::Databases);
                        });
                        screen.clear_filters(window, cx);
                        screen.model.toast("Database added.", cx);
                    }
                    Err(err) => {
                        let message = ui::i18n::t(&err.to_string()).to_owned();
                        if screen.add_database.is_none() {
                            screen.model.toast(message.clone(), cx);
                        }
                        screen.collection_error = Some(message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Opens the collection's settings and loads full statistics off the UI thread.
    pub(super) fn open_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.collection_busy() {
            return;
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let name = self
            .model
            .read(cx)
            .databases
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.name.to_string())
            .unwrap_or_default();
        self.manager = Some(Manager {
            reference: self.model.read(cx).config.explorer_default.as_ref() == Some(&path),
            path: path.clone(),
            name: new_input(name, "Name", window, cx),
            description: new_input("", "Description", window, cx),
            from: new_input("", "Player to replace", window, cx),
            into: new_input("", "Player to keep", window, cx),
            stats: None,
            bytes: 0,
            busy: true,
            error: None,
        });
        let worker = cx.background_spawn(async move {
            let database = Database::open(&path)?;
            let description = database.description()?;
            let stats = database.database_stats()?;
            let bytes = std::fs::metadata(&path)?.len();
            anyhow::Ok((path, description, stats, bytes))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |screen, window, cx| {
                if let Some(manager) = &mut screen.manager {
                    manager.busy = false;
                    match result {
                        Ok((path, description, stats, bytes)) if manager.path == path => {
                            manager
                                .description
                                .update(cx, |input, cx| input.set_value(description, window, cx));
                            manager.stats = Some(stats);
                            manager.bytes = bytes;
                        }
                        Err(err) => manager.error = Some(err.to_string()),
                        _ => {}
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Applies details or a player merge, or previews the precise games a cleanup would delete.
    fn manage_collection(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.collection_busy() {
            return;
        }
        let Some(manager) = &mut self.manager else {
            return;
        };
        let path = manager.path.clone();
        let name = manager.name.read(cx).value().trim().to_owned();
        let description = manager.description.read(cx).value().trim().to_owned();
        let from = manager.from.read(cx).value().trim().to_owned();
        let into = manager.into.read(cx).value().trim().to_owned();
        let reference = manager.reference;
        if matches!(action, Action::Save) && name.is_empty()
            || matches!(action, Action::Merge)
                && (from.is_empty() || into.is_empty() || from == into)
        {
            return;
        }
        manager.busy = true;
        manager.error = None;
        let fixed_name = path == self.model.read(cx).my_games_path();
        let worker = cx.background_spawn({
            let path = path.clone();
            async move {
                let mut database = Database::open(&path)?;
                match action {
                    Action::Save => {
                        database.set_details(
                            if fixed_name { "My games" } else { &name },
                            &description,
                        )?;
                        anyhow::Ok((0, Vec::new()))
                    }
                    Action::Merge => Ok((database.merge_player(&from, &into)?, Vec::new())),
                    Action::Duplicates => Ok((0, database.duplicate_game_ids()?)),
                    Action::Empty => Ok((0, database.empty_game_ids()?)),
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                if let Some(manager) = &mut screen.manager {
                    manager.busy = false;
                }
                match result {
                    Ok((count, ids)) => {
                        if matches!(action, Action::Save) {
                            screen.model.update(cx, |model, cx| {
                                if reference {
                                    model.config.explorer_default = Some(path.clone());
                                } else if model.config.explorer_default.as_ref() == Some(&path) {
                                    model.config.explorer_default = None;
                                }
                                model.save_config(cx);
                            });
                            screen.manager = None;
                        }
                        if matches!(action, Action::Merge) {
                            screen.manager = None;
                            screen.model.toast(
                                ui::i18n::tf("Updated {} games.", &[&format_count(count)]),
                                cx,
                            );
                        }
                        if matches!(action, Action::Duplicates | Action::Empty) {
                            if ids.is_empty() {
                                screen.model.toast("No matching games to remove.", cx);
                            } else {
                                screen.manager = None;
                                screen.selected_db = Some(path);
                                screen.apply_filter(cx);
                                screen.ask_delete(ids, cx);
                            }
                        }
                        screen.model.update(cx, |model, cx| {
                            model.refresh_databases();
                            model.refresh_leaks(cx);
                            cx.emit(ModelEvent::Databases);
                        });
                    }
                    Err(err) => {
                        if let Some(manager) = &mut screen.manager {
                            manager.error = Some(err.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Keeps an installation's progress and cancellation visible after its dialog closes.
    pub(super) fn render_collection_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.collection_control.is_none() || self.add_database.is_some() {
            return None;
        }
        let this = cx.entity();
        Some(
            h_flex()
                .gap(px(12.))
                .justify_between()
                .child(mono(self.collection_progress.clone()).text_size(px(13.)))
                .child(
                    btn("cancel-background-collection", "Cancel")
                        .compact()
                        .on_click(move |_, cx| {
                            this.update(cx, |screen, _| {
                                if let Some(control) = &screen.collection_control {
                                    control.cancel();
                                }
                            });
                        }),
                )
                .into_any_element(),
        )
    }

    /// Renders local and web sources, with download/import progress and a Cancel action.
    pub(super) fn render_add_database(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let add = self.add_database.as_ref()?;
        let this = cx.entity();
        let busy = self.collection_busy();
        let switch = this.clone();
        let mut body = v_flex()
            .p(px(16.))
            .gap(px(12.))
            .child(segmented(
                "add-database-source",
                vec![
                    (false, ui::i18n::t("Local").into(), busy),
                    (true, ui::i18n::t("Web").into(), busy),
                ],
                add.web,
                move |web, _, cx| {
                    switch.update(cx, |screen, cx| {
                        if let Some(add) = &mut screen.add_database {
                            add.web = web;
                        }
                        cx.notify();
                    });
                },
            ))
            .child(field("Name", text_input(&add.name)))
            .child(field("Description", text_input(&add.description)))
            .child(field(
                if add.web {
                    "PGN download URL"
                } else {
                    "Local file"
                },
                text_input(if add.web {
                    &add.web_source
                } else {
                    &add.source
                }),
            ))
            .child(div().text_as(13., T.muted).child(ui::i18n::t(if add.web {
                "Download a PGN, PGN.gz or ZIP file over HTTPS."
            } else {
                "Import PGN, PGN.gz or ZIP, or copy a Fast SQLite database."
            })));
        if !busy {
            body = body.child(
                h_flex().child(
                    btn("create-empty-database", "Create empty database")
                        .compact()
                        .on_click(this.listen(move |screen, window, cx| {
                            if let Some(add) = screen.add_database.take() {
                                let name = add.name.read(cx).value().to_string();
                                let description = add.description.read(cx).value().to_string();
                                screen.create_db(name, description, window, cx);
                            }
                        })),
                ),
            );
        }
        if !add.web && !busy {
            let this = this.clone();
            body = body.child(
                h_flex().child(btn("browse-collection", "Browse…").compact().on_click(
                    move |window, cx| {
                        this.update(cx, |screen, cx| screen.browse_collection(window, cx));
                    },
                )),
            );
        }
        if add.web && !busy {
            let install = this.clone();
            body = body.child(
                panel()
                    .p(px(12.))
                    .child(section_label("The Week in Chess"))
                    .child(div().my(px(8.)).text_size(px(13.)).child(ui::i18n::t(
                        "Download the latest weekly issue. Free for personal use.",
                    )))
                    .child(
                        btn("install-twic", "Install latest issue")
                            .compact()
                            .on_click(install.listen(move |screen, window, cx| {
                                screen.install_collection(true, window, cx)
                            })),
                    ),
            );
        }
        if let Some(error) = &self.collection_error {
            body = body.child(div().text_as(13., T.warn).child(error.clone()));
        }
        let act = this.clone();
        body = body.child(if busy {
            h_flex()
                .justify_between()
                .child(mono(self.collection_progress.clone()).text_size(px(12.)))
                .child(
                    btn("cancel-collection", "Cancel")
                        .compact()
                        .on_click(act.listen(move |screen, _, cx| {
                            if let Some(control) = &screen.collection_control {
                                control.cancel();
                            }
                            cx.notify();
                        })),
                )
        } else {
            h_flex()
                .justify_end()
                .child(btn("add-collection", "Add database").primary().on_click(
                    move |window, cx| {
                        act.update(cx, |screen, cx| {
                            screen.install_collection(false, window, cx)
                        });
                    },
                ))
        });
        Some(dialog(
            "add-database-dialog",
            "Add database",
            600.,
            body.into_any_element(),
            this,
        ))
    }

    /// Renders details, full-collection figures and explicit maintenance actions.
    pub(super) fn render_manager(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let manager = self.manager.as_ref()?;
        let busy = self.collection_busy();
        let this = cx.entity();
        let mut body = v_flex()
            .p(px(16.))
            .gap(px(12.))
            .child(if manager.path == self.model.read(cx).my_games_path() {
                field(
                    "Name",
                    div()
                        .h(px(34.))
                        .flex()
                        .items_center()
                        .child(ui::i18n::t("My games")),
                )
                .into_any_element()
            } else {
                field("Name", text_input(&manager.name)).into_any_element()
            })
            .child(field("Description", text_input(&manager.description)))
            .child({
                check(
                    "reference-database",
                    manager.reference,
                    "Use as reference database",
                )
                .on_change(this.listen_with(move |screen, value, _, cx| {
                    if let Some(manager) = &mut screen.manager {
                        manager.reference = value;
                    }
                    cx.notify();
                }))
            });
        if let Some(stats) = &manager.stats {
            body = body.child(
                h_flex()
                    .gap(px(24.))
                    .child(stat("Games", format_count(stats.games)))
                    .child(stat("Players", format_count(stats.players)))
                    .child(stat("Events", format_count(stats.events)))
                    .child(stat(
                        "Size",
                        format!("{:.1} MB", manager.bytes as f64 / 1_048_576.),
                    )),
            );
            body = body.child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .child(ui::i18n::t("Position index ready")),
                    )
                    .child({
                        btn(
                            "manage-structure-index",
                            if stats.structure_indexed || self.pattern_indexed {
                                "Rebuild structure index"
                            } else {
                                "Build structure index"
                            },
                        )
                        .compact()
                        .disabled(busy)
                        .on_click(this.listen(move |screen, _, cx| screen.build_pattern_index(cx)))
                    }),
            );
        } else {
            body = body.child(mono(ui::i18n::t("Loading…")));
        }
        let explore = btn("explore-collection", "Explore games")
            .compact()
            .disabled(busy)
            .on_click(this.listen(move |screen, window, cx| {
                screen.manager = None;
                screen.clear_filters(window, cx);
            }));
        let merge_hint = ui::i18n::t("Replace this exact player name on both sides of every game.");
        let merge_names = h_flex()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(text_input(&manager.from)))
            .child(div().flex_1().min_w_0().child(text_input(&manager.into)));
        let merge = btn("merge-players", "Merge players")
            .compact()
            .disabled(busy)
            .on_click(
                this.listen(move |screen, _, cx| screen.manage_collection(Action::Merge, cx)),
            );
        let clean_hint = ui::i18n::t(
            "Review the number of games before confirming removal. Different annotations are kept.",
        );
        let clean = [
            (Action::Duplicates, "Remove duplicates"),
            (Action::Empty, "Remove empty games"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (action, label))| {
            btn(("clean-collection", index), label)
                .compact()
                .disabled(busy)
                .on_click(this.listen(move |screen, _, cx| screen.manage_collection(action, cx)))
        });
        body = body
            .child(h_flex().child(explore))
            .child(section_label("Merge players"))
            .child(div().text_as(13., T.muted).child(merge_hint))
            .child(merge_names)
            .child(h_flex().child(merge))
            .child(section_label("Clean up"))
            .child(div().text_as(13., T.muted).child(clean_hint))
            .child(h_flex().gap(px(8.)).children(clean));
        if self.pattern_control.is_some() {
            let this = this.clone();
            body = body.child(
                btn("cancel-managed-index", "Cancel indexing")
                    .compact()
                    .on_click(move |_, cx| {
                        this.update(cx, |screen, _| {
                            if let Some(control) = &screen.pattern_control {
                                control.cancel();
                            }
                        });
                    }),
            );
            body = body.child(
                mono(ui::i18n::tf(
                    "Indexing {} / {} games",
                    &[
                        &self.pattern_progress.0.to_string(),
                        &self.pattern_progress.1.to_string(),
                    ],
                ))
                .text_size(px(12.)),
            );
        }
        if let Some(error) = &manager.error {
            body = body.child(div().text_as(13., T.warn).child(error.clone()));
        }
        body = body.child(h_flex().child({
            btn("remove-managed-database", "Remove database")
                .compact()
                .disabled(busy || manager.path == self.model.read(cx).my_games_path())
                .tooltip(ui::i18n::t("Move to the removed folder"))
                .on_click(this.listen(move |screen, _, cx| {
                    if let Some(manager) = screen.manager.take() {
                        screen
                            .model
                            .update(cx, |model, cx| model.remove_database(manager.path, cx));
                    }
                    cx.notify();
                }))
        }));
        body = body.child(
            h_flex()
                .justify_end()
                .gap(px(8.))
                .child({
                    btn("manage-add-games", "Add games")
                        .compact()
                        .disabled(busy)
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.manager = None;
                            screen.choose_import_file(cx);
                        }))
                })
                .child({
                    btn("manage-export", "Export PGN")
                        .compact()
                        .disabled(busy)
                        .on_click(this.listen(move |screen, _, cx| screen.export_collection(cx)))
                })
                .child({
                    btn("save-collection", "Save")
                        .primary()
                        .disabled(busy)
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.manage_collection(Action::Save, cx)
                        }))
                }),
        );
        Some(dialog(
            "manage-database-dialog",
            "Manage database",
            700.,
            body.into_any_element(),
            this,
        ))
    }

    /// Exports the whole collection regardless of the filters currently shown in the table.
    fn export_collection(&mut self, cx: &mut Context<Self>) {
        let Some(manager) = &self.manager else {
            return;
        };
        let path = manager.path.clone();
        let model = self.model.clone();
        let name = manager.name.read(cx).value().to_string();
        crate::model::save_pgn(model.clone(), &name, cx, move |destination, cx| {
            let worker = cx.background_spawn(async move {
                use std::io::Write as _;
                anyhow::ensure!(
                    destination.canonicalize().ok() != path.canonicalize().ok(),
                    "Choose a different file for PGN export"
                );
                let database = Database::open(&path)?;
                let parent = destination
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| std::path::Path::new("."));
                let temporary = tempfile::NamedTempFile::new_in(parent)?;
                let mut file = std::io::BufWriter::new(temporary);
                let ids = database.filtered_ids(&Filter::default())?;
                for id in &ids {
                    if let Some(game) = database.game(*id)? {
                        writeln!(file, "{}", game.to_pgn())?;
                    }
                }
                file.flush()?;
                file.into_inner()
                    .map_err(|err| err.into_error())?
                    .persist(destination)?;
                anyhow::Ok(ids.len())
            });
            cx.spawn(async move |cx| {
                let result = worker.await;
                model.update(cx, |model, cx| match result {
                    Ok(count) => model.toast(
                        ui::i18n::tf("Exported {} games.", &[&count.to_string()]),
                        cx,
                    ),
                    Err(err) => model.toast(err.to_string(), cx),
                });
            })
            .detach();
        });
    }
}
