//! Settings › Data: backups, online accounts and disk use.

use super::*;

/// The drive holding Fast's data: a bar of space used by other files, by Fast and free,
/// with the figures beside it.
pub(super) fn drive_strip(disk: crate::storage::DiskSpace, fast: u64) -> Div {
    use crate::format::format_bytes;
    let total = disk.total.max(1) as f32;
    let used = disk.total.saturating_sub(disk.free);
    let others = used.saturating_sub(fast);
    let figure = |caption: &'static str, value: String, color: u32| {
        v_flex()
            .flex_none()
            .gap(px(1.))
            .child(
                h_flex()
                    .gap(px(5.))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded(px(2.))
                            .bg(rgb(color))
                            .border_1()
                            .border_color(rgb(if color == T.divider {
                                T.border_strong
                            } else {
                                color
                            })),
                    )
                    .child(div().text_as(10., T.faint).child(ui::i18n::t(caption))),
            )
            .child(mono(value).whitespace_nowrap().text_as(12., T.fg_2))
    };
    h_flex()
        .px(px(16.))
        .py(px(12.))
        .gap(px(20.))
        .border_b_1()
        .border_color(rgb(T.divider))
        .child(
            h_flex()
                .flex_1()
                .min_w(px(120.))
                .h(px(8.))
                .rounded(px(2.))
                .overflow_hidden()
                .bg(rgb(T.divider))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(others as f32 / total))
                        .bg(rgb(T.muted)),
                )
                // Fast's share is often tiny, so it always shows at least a sliver.
                .child(
                    div()
                        .h_full()
                        .min_w(px(3.))
                        .w(gpui::relative(fast as f32 / total))
                        .bg(rgb(T.accent)),
                ),
        )
        .child(figure("FAST", format_bytes(fast), T.accent))
        .child(figure("OTHER FILES", format_bytes(others), T.muted))
        .child(figure("FREE", format_bytes(disk.free), T.divider))
        .child(
            v_flex()
                .flex_none()
                .gap(px(1.))
                .child(div().text_as(10., T.faint).child(ui::i18n::t("DRIVE")))
                .child(
                    mono(format_bytes(disk.total))
                        .whitespace_nowrap()
                        .text_as(12., T.fg_2),
                ),
        )
}

impl SettingsScreen {
    /// Measures disk use in the background; the Data page shows dashes until it is done.
    pub(super) fn measure_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage_task.is_some() {
            return;
        }
        let paths = self.model.read(cx).paths.clone();
        let worker = cx.background_spawn(async move { crate::storage::measure(&paths) });
        self.usage_task = Some(cx.spawn(async move |this, cx| {
            let usage = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.usage = Some(usage);
                screen.usage_task = None;
                cx.notify();
            });
        }));
    }

    /// Export everything to one backup file, or preview and restore one.
    pub(super) fn render_backup(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.clone();
        let export = model.clone();
        let import = model.clone();
        let mut body = v_flex().gap(px(12.)).child(
            h_flex()
                .gap(px(8.))
                .child(
                    btn("export-backup", "Export complete backup").on_click(move |_, cx| {
                        let target = export.clone();
                        crate::model::pick_path(
                            export.clone(),
                            "Export backup",
                            Some("fast-backup.json"),
                            cx,
                            move |path, cx| {
                                target.update(cx, |model, cx| {
                                    let paths = model.paths.clone();
                                    let config = model.config.clone();
                                    let repertoire = model.repertoire.clone();
                                    let task = cx.background_spawn(async move {
                                        crate::backup::Archive::capture(
                                            &paths,
                                            &config,
                                            &repertoire,
                                        )
                                        .and_then(|archive| Ok(serde_json::to_vec(&archive)?))
                                        .and_then(
                                            |bytes| crate::persistence::atomic_write(&path, &bytes),
                                        )
                                    });
                                    model.toast("Exporting backup…", cx);
                                    cx.spawn(async move |this, cx| {
                                        let result = task.await;
                                        let _ = this.update(cx, |model, cx| {
                                            model.toast(
                                                match result {
                                                    Ok(()) => "Backup exported".into(),
                                                    Err(err) => format!("Backup failed: {err:#}"),
                                                },
                                                cx,
                                            )
                                        });
                                    })
                                    .detach();
                                });
                            },
                        );
                    }),
                )
                .child(btn("preview-backup", "Preview backup to restore").on_click(
                    move |_, cx| {
                        let target = import.clone();
                        crate::model::pick_path(
                            import.clone(),
                            "Preview backup",
                            None,
                            cx,
                            move |path, cx| {
                                target.update(cx, |model, cx| {
                                    let task = cx.background_spawn(async move {
                                        crate::backup::Archive::read(&path)
                                    });
                                    model.toast("Reading backup preview…", cx);
                                    cx.spawn(async move |this, cx| {
                                        let result = task.await;
                                        let _ = this.update(cx, |model, cx| match result {
                                            Ok(archive) => {
                                                model.pending_backup = Some(archive);
                                                cx.notify();
                                            }
                                            Err(err) => model.toast(
                                                format!("Cannot restore backup: {err:#}"),
                                                cx,
                                            ),
                                        });
                                    })
                                    .detach();
                                });
                            },
                        );
                    },
                )),
        );
        if let Some(archive) = &model.read(cx).pending_backup {
            body = body.child(format!(
                "Backup preview ({} files): {}",
                archive.files.len(),
                archive
                    .files
                    .keys()
                    .map(|name| {
                        let paths = &model.read(cx).paths;
                        let target = if name == "config.toml" {
                            paths.config_file.clone()
                        } else {
                            paths.data_dir.join(name)
                        };
                        format!(
                            "{name} ({})",
                            if target.exists() {
                                "conflict: already exists"
                            } else {
                                "new"
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            for (id, label, replace) in [
                (
                    "restore-keep",
                    "Restore missing files; keep conflicts",
                    false,
                ),
                ("restore-replace", "Restore and replace conflicts", true),
            ] {
                let model = model.clone();
                body = body.child(btn(id, label).on_click(model.listen(move |model, _, cx| {
                    if let Some(archive) = model.pending_backup.take() {
                        match model.restore_backup(&archive, replace, cx) {
                            Ok(()) => {
                                model.config = crate::config::Config::load(&model.paths.config_file);
                                if let Ok(bytes) = std::fs::read(model.paths.repertoire_file()) && let Ok(rep) = serde_json::from_slice(&bytes) { model.repertoire = rep; }
                                model.notes = db::NoteStore::open(model.paths.notes_file()).ok();
                                model.refresh_databases();
                                cx.emit(crate::model::ModelEvent::Restored);
                                cx.emit(crate::model::ModelEvent::Repertoire);
                                cx.emit(crate::model::ModelEvent::Databases);
                                cx.emit(crate::model::ModelEvent::BoardPrefs);
                                model.toast("Backup restored. Restart Fast to reload all settings and study state.", cx);
                            }
                            Err(err) => { model.pending_backup = Some(archive); model.toast(format!("Restore failed: {err:#}"), cx); }
                        }
                    }
                })));
            }
            body = body.child(
                btn("cancel-restore", "Cancel restore").on_click(model.listen(
                    move |model, _, cx| {
                        model.pending_backup = None;
                        cx.notify();
                    },
                )),
            );
        }
        section("Backup").child(body)
    }

    pub(super) fn render_accounts(&mut self) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Accounts")))
            .child(section(ui::i18n::t("Offline by design")).child(
                div().text_color(rgb(T.fg_2)).child(ui::i18n::t(
                    "Fast has no accounts or cloud sync: everything stays on this computer. \
                     Only Analyse's Online panel, while open, sends the position on the board \
                     to chesstb-explorer.hf.space and chessdb.cn.",
                )),
            ))
    }

    /// The Data page: where Fast keeps its files, what each kind holds, its size, and backup.
    pub(super) fn render_data(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let paths = self.model.read(cx).paths.clone();
        let config_dir = crate::storage::config_dir(&paths);
        let usage = self.usage.clone();
        let measuring = self.usage_task.is_some();
        let size = |bytes: Option<u64>| {
            // Nothing measured yet, or nothing saved of that kind.
            bytes
                .filter(|bytes| *bytes > 0)
                .map_or_else(|| "—".to_owned(), crate::format::format_bytes)
        };
        let size_cell = |text: String| {
            mono(text)
                .w(px(96.))
                .flex_none()
                .whitespace_nowrap()
                .flex()
                .justify_end()
                .text_as(13., T.fg_2)
        };
        let row = |name: &str, about: &str, bytes: Option<u64>| {
            h_flex()
                .h(px(40.))
                .gap(px(12.))
                .border_b_1()
                .border_color(rgb(T.divider))
                .child(
                    div()
                        .w(px(160.))
                        .flex_none()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(ui::i18n::t(name).to_owned()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_as(13., T.muted)
                        .child(ui::i18n::t(about).to_owned()),
                )
                .child(size_cell(size(bytes)))
        };
        let mut table = v_flex().child(
            h_flex()
                .h(px(32.))
                .gap(px(12.))
                .px(px(16.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.divider))
                .child(div().w(px(160.)).flex_none().child(section_label("Data")))
                .child(div().flex_1().child(section_label("Contents")))
                .child(
                    h_flex()
                        .w(px(96.))
                        .flex_none()
                        .justify_end()
                        .child(section_label("Size")),
                ),
        );
        let mut rows = v_flex().px(px(16.));
        for (index, store) in crate::storage::STORES.iter().enumerate() {
            let bytes = usage.as_ref().map(|usage| usage.stores[index]);
            rows = rows.child(row(store.name, store.about, bytes));
        }
        if let Some(other) = usage
            .as_ref()
            .map(|usage| usage.other)
            .filter(|other| *other > 0)
        {
            rows = rows.child(row("Other", "Files Fast does not list above", Some(other)));
        }
        rows = rows.child(
            h_flex()
                .h(px(40.))
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(ui::i18n::t("Total")),
                )
                .child(
                    size_cell(size(usage.as_ref().map(crate::storage::Usage::total)))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(T.fg)),
                ),
        );
        table = table.child(rows);
        let drive = usage
            .as_ref()
            .and_then(|usage| Some((usage.disk?, usage.total())))
            .map(|(disk, fast)| drive_strip(disk, fast));
        let folder_row = |id: &'static str, label: &'static str, path: PathBuf| {
            h_flex()
                .h(px(44.))
                .gap(px(12.))
                .border_b_1()
                .border_color(rgb(T.divider))
                .child(
                    div()
                        .w(px(160.))
                        .flex_none()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(ui::i18n::t(label)),
                )
                .child(
                    mono(path.display().to_string())
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_as(12., T.fg_2),
                )
                .child(
                    btn(id, "Open folder")
                        .compact()
                        .disabled(!path.is_dir())
                        .on_click(move |_, _| crate::storage::reveal_folder(&path)),
                )
        };
        let backup = self.render_backup(cx);
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Data")))
            .child(
                section(ui::i18n::t("Folders"))
                    .child(folder_row("open-config-folder", "Settings", config_dir))
                    .child(folder_row(
                        "open-data-folder",
                        "Everything else",
                        paths.data_dir.clone(),
                    ))
                    .child(div().pt(px(10.)).text_as(12., T.muted).child(ui::i18n::t(
                        "Everything stays on this computer. Back up these folders to keep \
                                 your games, repertoire and analysis.",
                    ))),
            )
            .child(
                panel()
                    .overflow_hidden()
                    .child(
                        ui::widgets::panel_header("Disk use").child(
                            btn(
                                "measure-usage",
                                if measuring { "Measuring…" } else { "Refresh" },
                            )
                            .compact()
                            .disabled(measuring)
                            .on_click(this.listen(
                                move |screen, _, cx| {
                                    screen.measure_usage(cx);
                                    cx.notify();
                                },
                            )),
                        ),
                    )
                    .children(drive)
                    .child(table),
            )
            .child(backup)
    }
}
