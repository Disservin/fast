//! The list of databases: creating, renaming, duplicating and deleting them.

use super::*;

impl DatabaseScreen {
    /// Whether a database's name can be changed. "My games" keeps its name, since saving
    /// games from Analyse looks for it.
    pub(super) fn can_rename(&self, path: &PathBuf, cx: &gpui::App) -> bool {
        *path != self.model.read(cx).my_games_path()
    }

    /// Opens a field in the database's place in the list to type a new name for it.
    pub(super) fn start_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_rename(&path, cx) {
            return;
        }
        let Some(entry) = self
            .model
            .read(cx)
            .databases
            .iter()
            .find(|entry| entry.path == path)
        else {
            return;
        };
        let current = entry.name.to_string();
        self.renaming = Some(crate::controls::Rename::start(
            path,
            current,
            window,
            cx,
            |this, window, cx| this.finish_rename(true, window, cx),
        ));
        cx.notify();
    }

    /// Closes the rename field, storing the typed name when `keep` is set and it is not
    /// empty.
    pub(super) fn finish_rename(
        &mut self,
        keep: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        cx.notify();
        let name = rename.text(cx);
        let path = rename.key;
        let unchanged = self
            .model
            .read(cx)
            .databases
            .iter()
            .any(|entry| entry.path == path && entry.name == name);
        if !keep || name.is_empty() || unchanged {
            return;
        }
        let model = self.model.clone();
        let task =
            cx.background_spawn(
                async move { Database::open(&path).and_then(|db| db.set_name(&name)) },
            );
        cx.spawn(async move |_, cx| {
            let result = task.await;
            model.update(cx, |model, cx| {
                if let Err(err) = result {
                    model.toast(format!("Could not rename the database: {err}"), cx);
                }
                model.refresh_databases();
                cx.emit(ModelEvent::Databases);
            });
        })
        .detach();
    }

    /// Creates a collection from the Add dialog's details, asking for a name when omitted.
    pub(super) fn create_db(
        &mut self,
        name: String,
        description: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rename = name.trim().is_empty();
        let name = if rename {
            ui::i18n::t("New database").to_owned()
        } else {
            name.trim().to_owned()
        };
        let dir = self.model.read(cx).paths.databases_dir();
        let task = cx.background_spawn(async move {
            std::fs::create_dir_all(&dir)?;
            for number in 1_u64.. {
                let path = dir.join(format!("custom-{number}.sqlite"));
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(_) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(err) => return Err(err.into()),
                }
                Database::open(&path)?.set_details(&name, &description)?;
                return Ok::<_, anyhow::Error>(path);
            }
            unreachable!("an unused database file name")
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(path) => {
                    this.model.update(cx, |model, _| model.refresh_databases());
                    this.show_newest(path.clone(), window, cx);
                    if rename {
                        this.start_rename(path, window, cx);
                    }
                    this.model
                        .update(cx, |_, cx| cx.emit(ModelEvent::Databases));
                }
                Err(err) => this.model.toast(
                    format!("{}: {err}", ui::i18n::t("Could not create the database")),
                    cx,
                ),
            });
        })
        .detach();
    }

    /// Copies a database to a new file next to it, named "<name> copy", and selects
    /// the copy.
    pub(super) fn duplicate_db(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let Some(entry) = model.databases.iter().find(|entry| entry.path == path) else {
            return;
        };
        let name = format!("{} {}", entry.name, ui::i18n::t("copy"));
        let dir = model.paths.databases_dir();
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "database".to_owned());
        let dest = (1..)
            .map(|n| {
                let suffix = if n == 1 {
                    String::new()
                } else {
                    format!("-{n}")
                };
                dir.join(format!("{stem}-copy{suffix}.sqlite"))
            })
            .find(|candidate| !candidate.exists())
            .expect("an unused file name");
        let task = cx.background_spawn({
            let dest = dest.clone();
            async move { Database::open(&path)?.duplicate_to(&dest, &name) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.model.update(cx, |model, cx| {
                    match &result {
                        Ok(()) => model.toast("Duplicated the database.", cx),
                        Err(err) => {
                            model.toast(format!("Could not duplicate the database: {err}"), cx)
                        }
                    }
                    model.refresh_databases();
                });
                if result.is_ok() {
                    this.selected_db = Some(dest);
                    this.selected = None;
                }
                this.model
                    .update(cx, |_, cx| cx.emit(ModelEvent::Databases));
            });
        })
        .detach();
        self.model.toast("Duplicating the database…", cx);
    }

    /// Asks before deleting games from the selected database.
    pub(super) fn ask_delete(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        if !ids.is_empty() {
            self.confirm_delete = Some(ids);
            cx.notify();
        }
    }

    /// Deletes the games the user confirmed, then shows the next game.
    pub(super) fn delete_confirmed(&mut self, cx: &mut Context<Self>) {
        let (Some(ids), Some(path)) = (self.confirm_delete.take(), self.selected_db.clone()) else {
            return;
        };
        // Move the selection to the game after the deleted ones, so the table stays put.
        if let Some(selected) = self.selected
            && ids.contains(&selected)
        {
            let index = self.ids.iter().position(|&id| id == selected).unwrap_or(0);
            self.selected = self.ids[index..]
                .iter()
                .chain(self.ids[..index].iter().rev())
                .find(|id| !ids.contains(id))
                .copied();
        }
        cx.notify();
        let task = cx.background_spawn(async move { Database::open(&path)?.delete_games(&ids) });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let result = task.await;
            model.update(cx, |model, cx| {
                match result {
                    Ok(1) => model.toast("Deleted 1 game.", cx),
                    Ok(count) => model.toast(format!("Deleted {} games.", format_count(count)), cx),
                    Err(err) => model.toast(format!("Could not delete: {err}"), cx),
                }
                model.refresh_databases();
                model.refresh_leaks(cx);
                cx.emit(ModelEvent::Databases);
            });
        })
        .detach();
    }

    /// Keeps the searchable picker aligned with collections added, renamed or selected elsewhere.
    pub(super) fn sync_collection_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.model.read(cx).databases.clone();
        if self.collection_entries != entries {
            let mut used = std::collections::HashSet::new();
            let labels: Vec<SharedString> = entries
                .iter()
                .map(|entry| {
                    let mut label = entry.name.to_string();
                    let mut number = 2;
                    while !used.insert(label.clone()) {
                        label = format!("{} ({number})", entry.name);
                        number += 1;
                    }
                    label.into()
                })
                .collect();
            self.collection_picker.update(cx, |picker, cx| {
                picker.set_items(
                    gpui_component::select::SearchableVec::new(labels.clone()),
                    window,
                    cx,
                );
            });
            self.collection_entries = entries;
            self.collection_labels = labels;
        }
        let selected = self
            .collection_entries
            .iter()
            .position(|entry| Some(&entry.path) == self.selected_db.as_ref())
            .map(gpui_component::IndexPath::new);
        if self.collection_picker.read(cx).selected_index(cx) != selected {
            self.collection_picker.update(cx, |picker, cx| {
                picker.set_selected_index(selected, window, cx)
            });
        }
    }

    /// Shows the selected collection and its settings without taking a row per database.
    pub(super) fn render_collections(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let busy = self.collection_busy();
        let rename_cancel = this.clone();
        h_flex()
            .min_h(px(56.))
            .flex_none()
            .w_full()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .bg(rgb(T.surface))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(section_label("Database"))
            .child(
                div()
                    .w(px(280.))
                    .min_w_0()
                    .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            rename_cancel.update(cx, |screen, cx| {
                                if screen.renaming.is_some() {
                                    screen.finish_rename(false, window, cx);
                                }
                            });
                        }
                    })
                    .child(if let Some(rename) = &self.renaming {
                        text_input(&rename.input).into_any_element()
                    } else {
                        div()
                            .bg(rgb(T.bg))
                            .rounded(px(4.))
                            .child(
                                gpui_component::select::Select::new(&self.collection_picker)
                                    .h(px(34.))
                                    .text_size(px(13.5))
                                    .placeholder(ui::i18n::t("Choose database"))
                                    .search_placeholder(ui::i18n::t("Search databases")),
                            )
                            .into_any_element()
                    }),
            )
            .child(
                mono(self.db_snapshot.as_ref().map_or_else(
                    || "—".to_owned(),
                    |entry| ui::i18n::tf("{} games", &[&format_count(entry.games)]),
                ))
                .text_as(13., T.muted)
                .whitespace_nowrap(),
            )
            .child(div().flex_1())
            .child({
                btn("add-database", "Add database")
                    .compact()
                    .primary()
                    .disabled(busy && self.collection_control.is_none())
                    .on_click(
                        this.listen(move |screen, window, cx| screen.open_add_database(window, cx)),
                    )
            })
            .child({
                btn("manage-database", "Manage")
                    .compact()
                    .disabled(self.selected_db.is_none() || busy)
                    .on_click(
                        this.listen(move |screen, window, cx| screen.open_manager(window, cx)),
                    )
            })
    }

    /// Asks whether to delete the chosen games, saying how many and from where.
    pub(super) fn render_confirm_delete(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ids = self.confirm_delete.as_ref()?;
        let this = cx.entity();
        let database = self.db_snapshot.as_ref().map_or_else(
            || SharedString::from("this database"),
            |entry| entry.name.clone(),
        );
        let question = if ids.len() == 1 {
            format!("Delete this game from {database}?")
        } else {
            format!(
                "Delete {} games from {database}?",
                format_count(ids.len() as u64)
            )
        };
        let (cancel, confirm) = (this.clone(), this.clone());
        let body = v_flex()
            .child(
                v_flex()
                    .px(px(16.))
                    .py(px(14.))
                    .gap(px(4.))
                    .child(div().text_size(px(14.)).child(question))
                    .child(
                        div()
                            .text_as(13., T.muted)
                            .child(ui::i18n::t("Their reviews go too. This cannot be undone.")),
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
                    .child(btn("delete-cancel", "Cancel").on_click(cancel.listen(
                        move |screen, _, cx| {
                            screen.confirm_delete = None;
                            cx.notify();
                        },
                    )))
                    .child(btn("delete-confirm", "Delete").primary().on_click(
                        confirm.listen(move |screen, _, cx| screen.delete_confirmed(cx)),
                    )),
            );
        Some(dialog(
            "confirm-delete",
            if ids.len() == 1 {
                "Delete game"
            } else {
                "Delete games"
            },
            420.,
            body.into_any_element(),
            this,
        ))
    }
}
