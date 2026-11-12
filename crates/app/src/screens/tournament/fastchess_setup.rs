//! Getting fastchess: downloading it, choosing its path, and the folder that keeps tournament files.

use super::*;

impl TournamentScreen {
    /// Measures the tournaments folder in the background.
    pub(super) fn measure_folder(&mut self, cx: &mut Context<Self>) {
        let root = fastchess::tournaments_dir(&self.data_dir(cx));
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let size = cx
                .background_executor()
                .spawn(async move { fastchess::dir_size(&root) })
                .await;
            this.update(cx, |screen, cx| {
                screen.folder_size = Some(size);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Deletes the saved tournaments, keeping the one loaded in the tab.
    pub(super) fn clear_folder(&mut self, cx: &mut Context<Self>) {
        self.clear_armed = false;
        if self.run.as_ref().is_some_and(Run::active) {
            return;
        }
        let data = self.data_dir(cx);
        let keep = self.run.as_ref().map(|run| run.dir.clone());
        let model = self.model.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let (result, data) = cx
                .background_executor()
                .spawn(async move {
                    let result = fastchess::clear_tournaments(&data, keep.as_deref());
                    (result, data)
                })
                .await;
            this.update(cx, |screen, cx| {
                match result {
                    Ok((count, freed)) => model.toast(
                        format!(
                            "Deleted {count} tournament folder{}, freed {}",
                            if count == 1 { "" } else { "s" },
                            crate::format::format_bytes(freed)
                        ),
                        cx,
                    ),
                    Err(err) => model.toast(format!("Could not clear the folder: {err:#}"), cx),
                }
                screen.saved = fastchess::saved_runs(&data);
                screen.measure_folder(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn download(&mut self, cx: &mut Context<Self>) {
        if self.install_cancel.is_some() {
            return;
        }
        let data = self.data_dir(cx);
        let cancel = Arc::new(AtomicBool::new(false));
        self.install_cancel = Some(cancel.clone());
        self.install_status = Some(format!("Preparing fastchess {}…", fastchess::VERSION).into());
        self.install_task = Some(crate::event_reader::spawn_with_progress(
            cx,
            move |report| fastchess::install(&data, cancel, report),
            |screen, message, cx| {
                screen.install_status = Some(message.into());
                cx.notify();
            },
            |screen: &mut Self, result, cx| {
                screen.install_cancel = None;
                match result {
                    Ok(program) => {
                        screen.install_status =
                            Some(format!("fastchess {} is ready.", fastchess::VERSION).into());
                        if let Some(launch) = screen.pending.take() {
                            screen.spawn(program, launch, cx);
                        }
                    }
                    Err(err) => {
                        screen.pending = None;
                        screen.install_status =
                            Some(format!("Download stopped: {err:#}. You can retry.").into());
                    }
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Stops listening to the download at once; the worker notices the flag at its next
    /// step and installs nothing.
    pub(super) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.install_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            self.install_task = None;
            self.install_status = Some(ui::i18n::t("Download cancelled.").into());
        }
        self.pending = None;
        cx.notify();
    }

    pub(super) fn save_fastchess_path(&mut self, cx: &mut Context<Self>) {
        let text = self.text(&self.fastchess_path, cx);
        let path = (!text.is_empty()).then(|| PathBuf::from(text));
        if let Some(path) = &path
            && !path.is_file()
        {
            return self.fail(format!("No file at {}", path.display()), cx);
        }
        self.form_error = None;
        self.model
            .edit_config(cx, |config| config.fastchess_path = path);
        cx.notify();
    }

    pub(super) fn render_fastchess(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let installed = fastchess::installed(&self.data_dir(cx)).is_some();
        let custom = !self.text(&self.fastchess_path, cx).is_empty();
        let status = if custom {
            "Using your own fastchess executable.".to_owned()
        } else if installed {
            format!("fastchess {} is downloaded.", fastchess::VERSION)
        } else {
            format!(
                "fastchess {} is downloaded from GitHub the first time you start a tournament.",
                fastchess::VERSION
            )
        };
        let this = cx.entity();
        let save = this.clone();
        let cancel = this.clone();
        v_flex()
            .gap(px(8.))
            .child(section_label("fastchess"))
            .child(
                div()
                    .text_as(13., T.muted)
                    .child(SharedString::from(status)),
            )
            .children(
                self.install_status
                    .clone()
                    .map(|status| div().text_as(13., T.fg_2).child(status)),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(
                        !installed && !custom && self.install_cancel.is_none(),
                        |row| {
                            row.child(
                                btn("fastchess-download", ui::i18n::t("Download now")).on_click(
                                    this.listen(move |screen, _, cx| screen.download(cx)),
                                ),
                            )
                        },
                    )
                    .when(self.install_cancel.is_some(), |row| {
                        row.child(
                            btn("fastchess-cancel", ui::i18n::t("Cancel download")).on_click(
                                cancel.listen(move |screen, _, cx| screen.cancel_download(cx)),
                            ),
                        )
                    }),
            )
            .child(field(
                "Own executable (optional)",
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_input(&self.fastchess_path)),
                    )
                    .child(crate::controls::browse_path(
                        "fastchess-browse",
                        &self.model,
                        &self.fastchess_path,
                        "Engine executable",
                        false,
                    ))
                    .child(btn("fastchess-path-save", ui::i18n::t("Use")).on_click(
                        save.listen(move |screen, _, cx| screen.save_fastchess_path(cx)),
                    )),
            ))
    }

    /// The tournaments folder with its disk usage, and a button that deletes what is in it.
    pub(super) fn render_folder(&self, folder: PathBuf, cx: &mut Context<Self>) -> Div {
        let size = match self.folder_size {
            Some(size) => crate::format::format_bytes(size),
            None => "measuring…".to_owned(),
        };
        let busy = self.run.as_ref().is_some_and(Run::active);
        let has_data = self.folder_size.is_some_and(|size| size > 0);
        let this = cx.entity();
        let actions =
            if self.clear_armed {
                let cancel = this.clone();
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(div().text_as(12., T.warn).child(SharedString::from(format!(
                        "Delete {} of saved games and configs?",
                        size
                    ))))
                    .child(
                        btn("tournament-clear-confirm", "Delete")
                            .compact()
                            .on_click(this.listen(move |screen, _, cx| screen.clear_folder(cx))),
                    )
                    .child(btn("tournament-clear-cancel", "Cancel").compact().on_click(
                        cancel.listen(move |screen, _, cx| {
                            screen.clear_armed = false;
                            cx.notify();
                        }),
                    ))
            } else {
                h_flex().child(
                    btn("tournament-clear", "Free up space")
                        .compact()
                        .disabled(busy || !has_data)
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.clear_armed = true;
                            cx.notify();
                        })),
                )
            };
        v_flex()
            .gap(px(6.))
            .child(mono(format!("Folder: {} ({size})", folder.display())).text_as(12., T.muted))
            .child(actions)
    }
}
