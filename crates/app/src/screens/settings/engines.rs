//! Settings › Engines: the engine list, adding and removing engines, and the download catalogue.

use super::*;

/// A picture shipped next to the engine binary: `<name>.png`, `logo.png`, `icon.svg`…
pub fn find_icon(engine: &std::path::Path) -> Option<PathBuf> {
    let dir = engine.parent()?;
    let stem = engine.file_stem()?.to_string_lossy().into_owned();
    let names = [stem.as_str(), "logo", "icon"];
    names.iter().find_map(|name| {
        ["png", "svg", "jpg", "jpeg"].iter().find_map(|ext| {
            let candidate = dir.join(format!("{name}.{ext}"));
            candidate.is_file().then_some(candidate)
        })
    })
}

impl SettingsScreen {
    /// Builds the input states for an engine's options from its stored schema.
    pub fn select_engine(
        &mut self,
        id: Option<EngineId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_engine = id;
        self.benchmark = None;
        self.option_inputs.clear();
        self.option_selects.clear();
        self._option_subscriptions.clear();
        let Some(config) = id.and_then(|id| self.model.read(cx).engine(id).cloned()) else {
            cx.notify();
            return;
        };
        self.path_input.update(cx, |input, cx| {
            input.set_value(config.path.display().to_string(), window, cx);
        });
        self.name_input.update(cx, |input, cx| {
            input.set_value(config.name.clone(), window, cx);
        });
        for option in &config.info.options {
            let current = config.options.get(&option.name).cloned();
            match &option.kind {
                OptionKind::Spin { default, .. } => {
                    self.add_text_option(
                        &option.name,
                        current.unwrap_or(default.to_string()),
                        window,
                        cx,
                    );
                }
                OptionKind::String { default } => {
                    self.add_text_option(
                        &option.name,
                        current.unwrap_or(default.clone()),
                        window,
                        cx,
                    );
                }
                OptionKind::Combo { default, choices } => {
                    let value = current.unwrap_or(default.clone());
                    let index = choices
                        .iter()
                        .position(|choice| *choice == value)
                        .unwrap_or(0);
                    let items: Vec<SharedString> =
                        choices.iter().map(|choice| choice.clone().into()).collect();
                    let select = new_select(items, index, window, cx);
                    let name = option.name.clone();
                    let choices = choices.clone();
                    self._option_subscriptions.push(cx.subscribe(
                        &select,
                        move |this, state, _: &gpui_component::select::SelectEvent<_>, cx| {
                            let index = state
                                .read(cx)
                                .selected_index(cx)
                                .map_or(0, |index| index.row);
                            if let Some(value) = choices.get(index) {
                                this.set_engine_option(&name, value.clone(), cx);
                            }
                        },
                    ));
                    self.option_selects.insert(option.name.clone(), select);
                }
                OptionKind::Check { .. } | OptionKind::Button => {}
            }
        }
        cx.notify();
    }

    /// Turns UCI debug mode on or off and, when on, opens a window with the raw output.
    pub(super) fn set_debug(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            config.debug = on;
            if let Some(handle) = model.manager.get(id) {
                handle.set_debug(on);
            }
            model.save_config(cx);
        });
        if on {
            self.open_debug_window(cx);
        }
    }

    pub(super) fn open_debug_window(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        crate::model::open_debug_window(&self.model, id, cx);
    }

    pub(super) fn set_engine_use(
        &mut self,
        change: impl FnOnce(&mut EngineConfig),
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.set_engine_use(id, change, cx));
    }

    pub(super) fn add_from_file(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Add engine",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |this, cx| this.register_engine(path, cx));
            },
        );
    }

    pub(super) fn register_engine(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.probe_engine(path, None, cx);
    }

    pub(super) fn choose_engine_path(
        &mut self,
        option: Option<String>,
        directory: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let this = cx.entity().downgrade();
        let callback = move |path: PathBuf, cx: &mut gpui::App| {
            let Some(this) = this.upgrade() else { return };
            if let Some(option) = option {
                let value = path.to_string_lossy().into_owned();
                this.update(cx, |screen, cx| {
                    screen.model.update(cx, |model, cx| {
                        let Some(config) = model.engine_mut(id) else {
                            return;
                        };
                        if config.option_value(&option).as_ref() == Some(&value) {
                            return;
                        }
                        config.options.insert(option.clone(), value.clone());
                        if let Some(handle) = model.manager.get(id) {
                            handle.set_option(option.clone(), Some(value.clone()));
                        }
                        model.save_config(cx);
                        cx.emit(ModelEvent::Engines);
                    });
                });
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        this.update(cx, |screen, cx| screen.select_engine(Some(id), window, cx))
                    });
                }
            } else {
                this.update(cx, |screen, cx| screen.probe_engine(path, Some(id), cx));
            }
        };
        if directory {
            crate::model::pick_directory(
                self.model.clone(),
                "Select tablebase directory",
                cx,
                callback,
            );
        } else {
            pick_path(self.model.clone(), "Select engine file", None, cx, callback);
        }
    }

    pub(super) fn probe_engine(
        &mut self,
        path: PathBuf,
        replace: Option<EngineId>,
        cx: &mut Context<Self>,
    ) {
        self.probe_engine_defaults(path, replace, Default::default(), cx);
    }

    /// Checks a local engine with time for its first launch and retains startup errors.
    pub(super) fn probe_engine_defaults(
        &mut self,
        path: PathBuf,
        replace: Option<EngineId>,
        defaults: std::collections::BTreeMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        if self.probing_engine {
            return;
        }
        self.probing_engine = true;
        cx.notify();
        let handle = EngineHandle::spawn(path.clone(), Vec::new(), Vec::new());
        let this = cx.entity().downgrade();
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let result = handle.probe(std::time::Duration::from_secs(60)).await;
            let _ = this.update(cx, |screen, cx| {
                screen.probing_engine = false;
                cx.notify();
            });
            if let Err(reason) = &result {
                let _ = this.update(cx, |screen, cx| {
                    screen.install_status = Some(format!(
                        "{}: {reason}",
                        ui::i18n::t("Engine check failed. Retry using the installed files")
                    ).into());
                    cx.notify();
                });
            }
            let added = model.update(cx, |model, cx| match result {
                Ok(info) => {
                    let id = replace.unwrap_or(model.config.next_engine_id);
                    if replace.is_none() {
                        model.config.next_engine_id += 1;
                    }
                    let fallback = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_else(|| ui::i18n::t("Engine").to_owned());
                    let (name, version) = split_engine_name(&info.name, &fallback);
                    let wdl = info
                        .options
                        .iter()
                        .any(|option| option.name == "UCI_ShowWDL");
                    let mut options = std::collections::BTreeMap::new();
                    if wdl {
                        options.insert("UCI_ShowWDL".to_owned(), "true".to_owned());
                    }
                    if info.options.iter().any(|option| option.name == "MultiPV") {
                        options.insert("MultiPV".to_owned(), "3".to_owned());
                    }
                    for (name, value) in defaults {
                        if info.options.iter().any(|option| option.name == name) {
                            options.insert(name, value);
                        }
                    }
                    if let Some(existing) = replace.and_then(|id| model.engine_mut(id)) {
                        existing.path = path.clone();
                        existing.info = info;
                        existing.name = name;
                        existing.version = version;
                    } else if replace.is_some() {
                        model.toast("The selected engine was removed while choosing a file.", cx);
                        return None;
                    } else {
                        model.config.engines.push(EngineConfig {
                            id,
                            name,
                            version,
                            path: path.clone(),
                            enabled: true,
                            options,
                            use_analysis: true,
                            use_review: true,
                            use_play: true,
                            icon: find_icon(&path),
                            info,
                            ..Default::default()
                        });
                    }
                    model.engine_status.insert(id, EngineStatus::Ready);
                    model.save_config(cx);
                    cx.emit(ModelEvent::Engines);
                    Some(id)
                }
                Err(reason) => {
                    model.toast(format!("Could not add {}: {reason}", path.display()), cx);
                    None
                }
            });
            if let (Some(id), Some(this)) = (added, this.upgrade()) {
                cx.update(|cx| {
                    if let Some(window) = cx.active_window() {
                        let _ = window.update(cx, |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.select_engine(Some(id), window, cx);
                                this.install_status = Some(
                                    ui::i18n::t("Engine ready. Select it for analysis, review or play below.")
                                        .into(),
                                );
                            });
                        });
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn remove_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            model.manager.shutdown(id);
            model.config.engines.retain(|engine| engine.id != id);
            model.engine_status.remove(&id);
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
        let next = self
            .model
            .read(cx)
            .config
            .engines
            .first()
            .map(|engine| engine.id);
        self.select_engine(next, window, cx);
    }

    pub(super) fn restart_engine(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            model.manager.shutdown(id);
            model.engine_status.insert(id, EngineStatus::Ready);
            cx.emit(ModelEvent::Engines);
            cx.notify();
        });
    }

    pub(super) fn download_engine(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.install_cancel.is_some() || self.probing_engine {
            return;
        }
        let entry = match crate::engine_catalog::Catalog::bundled().and_then(|catalog| {
            catalog
                .engines
                .get(index)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown engine"))
        }) {
            Ok(entry) => entry,
            Err(err) => {
                self.model
                    .toast(format!("Cannot load engine catalog: {err:#}"), cx);
                return;
            }
        };
        let data = self.model.read(cx).paths.data_dir.clone();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.install_cancel = Some(cancel.clone());
        self.install_status = Some(format!("Preparing {}…", entry.name).into());
        let defaults = entry.defaults.clone();
        self.install_task = Some(crate::event_reader::spawn_with_progress(
            cx,
            move |report| crate::engine_catalog::install(&entry, &data, cancel, report),
            |screen, message, cx| {
                screen.install_status = Some(message.into());
                cx.notify();
            },
            |screen: &mut Self, result, cx| {
                screen.install_cancel = None;
                match result {
                    Ok(path) => {
                        screen.install_status =
                            Some(ui::i18n::t("Installed. Checking UCI handshake…").into());
                        let existing = screen
                            .model
                            .read(cx)
                            .config
                            .engines
                            .iter()
                            .find(|engine| engine.path == path)
                            .map(|engine| engine.id);
                        screen.probe_engine_defaults(path, existing, defaults, cx);
                    }
                    Err(err) => {
                        let message =
                            format!("Engine installation stopped: {err:#}. You can retry.");
                        screen.install_status = Some(message.clone().into());
                        screen.model.toast(message, cx);
                    }
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Stops listening to the install at once. The worker may be blocked on the network
    /// for a while; it sees the flag at its next step, and its staging folder is removed
    /// without anything being installed.
    pub(super) fn cancel_install(&mut self, cx: &mut Context<Self>) {
        let Some(cancel) = self.install_cancel.take() else {
            return;
        };
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        self.install_task = None;
        self.install_status = Some(ui::i18n::t("Download cancelled.").into());
        cx.notify();
    }

    pub(super) fn render_catalog(&self, cx: &mut Context<Self>) -> Div {
        let mut body = v_flex()
            .gap(px(12.))
            .child(section_label("Download an engine"))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(
                        "Downloads require an internet connection. Installed engines run offline.",
                    )),
            );
        let busy = self.install_cancel.is_some() || self.probing_engine;
        match crate::engine_catalog::Catalog::bundled() {
            Err(err) => body = body.child(format!("Engine catalog unavailable: {err:#}")),
            Ok(catalog) => {
                for (index, entry) in catalog.engines.into_iter().enumerate() {
                    let this = cx.entity();
                    let supported = entry.native_download().is_some();
                    let installed = entry.native_download().is_some_and(|asset| {
                        let path = entry
                            .install_dir(&self.model.read(cx).paths.data_dir, asset)
                            .join(&asset.executable);
                        self.model
                            .read(cx)
                            .config
                            .engines
                            .iter()
                            .any(|engine| engine.path == path)
                    });
                    let downloaded = entry.native_download().is_some_and(|asset| {
                        entry
                            .install_dir(&self.model.read(cx).paths.data_dir, asset)
                            .join(&asset.executable)
                            .is_file()
                    });
                    let homepage = entry.homepage.clone();
                    let source = entry.source_url.clone();
                    body = body.child(h_flex().gap(px(16.)).items_start()
                    .child(v_flex().flex_1().min_w_0().gap(px(4.))
                        .child(h_flex().gap(px(8.)).child(format!("{} {}", entry.name, entry.version)).child(badge(entry.license)))
                        .child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(entry.description))
                        .when(!supported, |column| column.child(format!("No catalog build for {} / {}. You can still add a local executable.", std::env::consts::OS, std::env::consts::ARCH))))
                    .child(btn(("engine-site", index), "Website").h(30.).on_click(move |_, cx| cx.open_url(&homepage)))
                    .child(btn(("engine-source", index), ui::i18n::t("Source")).h(30.).on_click(move |_, cx| cx.open_url(&source)))
                    .child(btn(("engine-install", index), if installed { ui::i18n::t("Installed") } else if downloaded { ui::i18n::t("Retry engine check") } else { ui::i18n::t("Download & install") }).h(30.).disabled(busy || !supported || installed)
                        .on_click(move |_, cx| this.update(cx, |screen, cx| screen.download_engine(index, cx)))));
                }
            }
        }
        if let Some(status) = &self.install_status {
            body = body.child(div().text_size(px(13.)).child(status.clone()));
        }
        if self.install_cancel.is_some() {
            let this = cx.entity();
            body = body.child(
                btn("cancel-engine-install", ui::i18n::t("Cancel download"))
                    .on_click(move |_, cx| this.update(cx, |screen, cx| screen.cancel_install(cx))),
            );
        }
        panel().p(px(16.)).child(body)
    }

    pub(super) fn render_engines(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let add_this = this.clone();
        let model = self.model.read(cx);
        let header = h_flex()
            .justify_between()
            .child(page_title(ui::i18n::t("Engines")))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(btn("add-engine", ui::i18n::t("Add from file")).on_click(
                        move |_, cx| {
                            add_this.update(cx, |screen, cx| screen.add_from_file(cx));
                        },
                    ))
                    .child(
                        btn("download-engine", "Download engine")
                            .primary()
                            .on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |screen, cx| {
                                        screen.catalog_open = !screen.catalog_open;
                                        cx.notify();
                                    })
                                }
                            }),
                    ),
            );
        // Fractions of the table so it fits any window width.
        let col_engine = gpui::relative(0.40);
        let col_protocol = gpui::relative(0.115);
        let col_used = gpui::relative(0.395);
        let mut table = panel().overflow_hidden().child(
            h_flex()
                .h(px(32.))
                .px(px(16.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(
                    div()
                        .w(col_engine)
                        .child(section_label(ui::i18n::t("Engine"))),
                )
                .child(div().w(col_protocol).child(section_label("Protocol")))
                .child(div().w(col_used).child(section_label("Used for")))
                .child(section_label(ui::i18n::t("Status"))),
        );
        for (index, engine) in model.config.engines.iter().enumerate() {
            let selected = self.selected_engine == Some(engine.id);
            let id = engine.id;
            let select_this = this.clone();
            let status = model
                .engine_status
                .get(&id)
                .copied()
                .unwrap_or(EngineStatus::Ready);
            let (status_text, status_color) = match status {
                EngineStatus::Ready => ("Ready", T.good),
                EngineStatus::Loading => ("Loading", T.accent),
                EngineStatus::Searching => ("Searching", T.good),
                EngineStatus::Paused => ("Paused", T.accent),
                EngineStatus::Stopped => ("Stopped", T.muted),
                EngineStatus::Disabled => (ui::i18n::t("Disabled"), T.muted),
                EngineStatus::Error => ("Error", T.warn),
            };
            table = table.child(
                h_flex()
                    .id(("engine", index))
                    .relative()
                    .h(px(43.))
                    .px(px(16.))
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .when(selected, |row| row.bg(rgb(T.selected)))
                    .when(!selected, |row| row.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, window, cx| {
                        select_this.update(cx, |screen, cx| {
                            screen.select_engine(Some(id), window, cx);
                        });
                    })
                    .when(selected, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(rgb(T.accent)),
                        )
                    })
                    .child(
                        h_flex()
                            .w(col_engine)
                            .gap(px(4.))
                            .child(div().mr(px(6.)).child(engine_icon(engine, 24.)))
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .child(SharedString::from(engine.name.clone())),
                            )
                            .child(
                                mono(engine.version.clone())
                                    .text_size(px(13.))
                                    .text_color(rgb(T.muted)),
                            ),
                    )
                    .child(
                        mono("UCI")
                            .w(col_protocol)
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2)),
                    )
                    .child(
                        div()
                            .w(col_used)
                            .text_color(rgb(T.fg_2))
                            .child(engine.used_for()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(status_color))
                            .child(status_text),
                    ),
            );
        }
        if model.config.engines.is_empty() {
            table = table.child(
                div()
                    .p(px(20.))
                    .text_size(px(14.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(
                        "No engines yet. Add a UCI engine executable with Add from file.",
                    )),
            );
        }
        let detail = self
            .selected_engine
            .and_then(|id| model.engine(id).cloned())
            .map(|config| self.render_engine_detail(&config, window, cx));
        let leave = LeaveAnalyse::of(&self.model.read(cx).config);
        let background_model = self.model.clone();
        let background_row = v_flex()
            .gap(px(6.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(ui::i18n::t("Leaving an Analyse tab")),
                    )
                    .child(segmented(
                        "leave-analyse",
                        vec![
                            (
                                LeaveAnalyse::Keep,
                                ui::i18n::t("Keep analysing").into(),
                                false,
                            ),
                            (LeaveAnalyse::Pause, ui::i18n::t("Pause").into(), false),
                            (LeaveAnalyse::Stop, ui::i18n::t("Stop").into(), false),
                        ],
                        leave,
                        move |value, _, cx| {
                            background_model.update(cx, |model, cx| {
                                model.config.analyse_in_background = value == LeaveAnalyse::Keep;
                                model.config.pause_in_background = value == LeaveAnalyse::Pause;
                                model.save_config(cx);
                            })
                        },
                    )),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(match leave {
                        LeaveAnalyse::Keep => {
                            "The engines keep searching while a tab other than Analyse is \
                             shown. Switching to another Analyse tab hands them over."
                        }
                        LeaveAnalyse::Pause => {
                            "The search pauses and keeps its lines. The tab shows a pause \
                             mark and stays paused when you come back, until you resume (P)."
                        }
                        LeaveAnalyse::Stop => {
                            "The search stops and starts again from your position when you \
                             come back."
                        }
                    })),
            );
        v_flex()
            .gap(px(21.))
            .child(header)
            .when(self.catalog_open, |column| {
                column.child(self.render_catalog(cx))
            })
            .child(table)
            .child(background_row)
            .children(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_next_to_the_binary_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let engine = dir.path().join("lc0");
        std::fs::write(&engine, "").unwrap();
        assert_eq!(find_icon(&engine), None);
        std::fs::write(dir.path().join("logo.svg"), "<svg/>").unwrap();
        assert_eq!(find_icon(&engine), Some(dir.path().join("logo.svg")));
        std::fs::write(dir.path().join("lc0.png"), "").unwrap();
        assert_eq!(find_icon(&engine), Some(dir.path().join("lc0.png")));
    }
}
