//! Downloading a player's games from an online account.

use super::*;

impl DatabaseScreen {
    /// The saved account matching the site and the typed username, if any.
    pub(super) fn typed_account(
        &self,
        site: Site,
        cx: &gpui::App,
    ) -> Option<crate::config::Account> {
        let user = self.download_user.read(cx).value().trim().to_owned();
        self.model
            .read(cx)
            .config
            .accounts
            .iter()
            .find(|account| account.site == site && account.name.eq_ignore_ascii_case(&user))
            .cloned()
    }

    /// Starts the download set up in the Import dialog.
    pub(super) fn download_from_dialog(&mut self, cx: &mut Context<Self>) {
        let ImportSource::Site(site) = self.import_source else {
            return;
        };
        let user = self.download_user.read(cx).value().trim().to_owned();
        if user.is_empty() {
            self.download_error = Some(ui::i18n::t("Type a username first.").to_owned());
            cx.notify();
            return;
        }
        let now_ms = now_seconds() * 1000;
        const DAY_MS: i64 = 86_400_000;
        let since_ms = match self.download_since {
            Since::All => None,
            Since::LastYear => Some(now_ms - 365 * DAY_MS),
            Since::LastMonth => Some(now_ms - 30 * DAY_MS),
            Since::LastUpdate => self
                .typed_account(site, cx)
                .and_then(|account| account.last_game_ms),
        };
        let my_games = self.model.read(cx).my_games_path();
        let database = match (&self.selected_db, self.download_into_selected) {
            (Some(selected), true) => selected.clone(),
            _ => my_games,
        };
        if self.download_is_me {
            self.model
                .edit_config(cx, |config| config.add_player_alias(&user));
        }
        let clocks = self.download_clocks;
        self.start_download(site, user, since_ms, database, clocks, cx);
    }

    /// Updates a saved account with the games played since its last download.
    pub fn update_account(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(account) = self.model.read(cx).config.accounts.get(index).cloned() else {
            return;
        };
        self.start_download(
            account.site,
            account.name,
            account.last_game_ms,
            account.database,
            true,
            cx,
        );
    }

    /// Downloads a player's games in the background, saving the account's newest game
    /// after every committed batch so a cancelled download keeps its place.
    pub(super) fn start_download(
        &mut self,
        site: Site,
        user: String,
        since_ms: Option<i64>,
        database: PathBuf,
        clocks_and_evals: bool,
        cx: &mut Context<Self>,
    ) {
        if self.downloading.is_some() || self.importing.is_some() {
            return;
        }
        let model = self.model.read(cx);
        let database_name = model
            .databases
            .iter()
            .find(|entry| entry.path == database)
            .map_or_else(
                || SharedString::from(ui::i18n::t("My games")),
                |entry| entry.name.clone(),
            );
        let request = DownloadRequest {
            site,
            user: user.clone(),
            since_ms,
            clocks_and_evals,
            database: database.clone(),
            keep_annotations: model.config.import_keep_annotations,
        };
        let control = ImportControl::default();
        self.download_error = None;
        self.downloading = Some(Downloading {
            control: control.clone(),
            site,
            user: user.clone(),
            database: database.clone(),
            database_name: database_name.clone(),
            games: 0,
            duplicates: 0,
            months: None,
            waiting: None,
            phase: DownloadPhase::Connecting,
        });
        self.model.update(cx, |model, cx| {
            if model.config.account_mut(site, &user).is_none() {
                model.config.accounts.push(crate::config::Account {
                    site,
                    name: user.clone(),
                    database: database.clone(),
                    last_game_ms: None,
                    updated_at: None,
                });
                model.save_config(cx);
            }
        });
        let (events_tx, events_rx) = smol::channel::unbounded();
        let worker = cx.background_spawn(async move {
            let result = crate::game_download::download(
                &request,
                &Endpoints::default(),
                &control,
                |event| {
                    let _ = events_tx.try_send(event);
                },
            );
            if let Ok(db) = Database::open(&request.database)
                && request
                    .database
                    .file_stem()
                    .is_some_and(|stem| stem == "my_games")
            {
                let _ = db.set_name("My games");
            }
            result
        });
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events_rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    let Some(running) = &mut this.downloading else {
                        return;
                    };
                    match event {
                        DownloadEvent::Phase(phase) => {
                            running.phase = phase;
                            running.waiting = None;
                        }
                        DownloadEvent::Progress {
                            games,
                            duplicates,
                            months,
                            latest_game_ms,
                        } => {
                            running.games = games;
                            running.duplicates = duplicates;
                            running.months = months.or(running.months);
                            running.waiting = None;
                            let (site, user) = (running.site, running.user.clone());
                            if let Some(latest) = latest_game_ms {
                                this.model.update(cx, |model, cx| {
                                    if let Some(account) = model.config.account_mut(site, &user)
                                        && account.last_game_ms.is_none_or(|known| known < latest)
                                    {
                                        account.last_game_ms = Some(latest);
                                        model.schedule_config_save(cx);
                                    }
                                });
                            }
                        }
                        DownloadEvent::Waiting(seconds) => running.waiting = Some(seconds),
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let outcome = worker.await;
            let _ = this.update(cx, |this, cx| {
                let Some(running) = this.downloading.take() else {
                    return;
                };
                let message = match &outcome {
                    Ok(summary) => {
                        let (site, user) = (running.site, running.user.clone());
                        let latest = summary.latest_game_ms;
                        this.model.update(cx, |model, cx| {
                            if let Some(account) = model.config.account_mut(site, &user) {
                                account.updated_at = Some(now_seconds());
                                if let Some(latest) = latest
                                    && account.last_game_ms.is_none_or(|known| known < latest)
                                {
                                    account.last_game_ms = Some(latest);
                                }
                            }
                            model.save_config(cx);
                        });
                        let mut text = ui::i18n::tf(
                            "Downloaded {} new games ({} already in {})",
                            &[
                                &format_count(summary.games),
                                &format_count(summary.duplicates),
                                &running.database_name,
                            ],
                        );
                        if summary.cancelled {
                            text = format!("{} {text}", ui::i18n::t("Download cancelled."));
                        }
                        Some(text)
                    }
                    Err(err) => {
                        this.download_error = Some(err.to_string());
                        (!this.import_dialog).then(|| err.to_string())
                    }
                };
                let database = running.database.clone();
                let screen = cx.entity().downgrade();
                this.model.update(cx, |model, cx| {
                    model.refresh_databases(cx);
                    model.refresh_leaks(cx);
                    if let Some(message) = message {
                        if outcome.is_ok() {
                            model.toast_with_action(
                                message,
                                "Show games",
                                move |window, cx| {
                                    let database = database.clone();
                                    let _ = screen.update(cx, |screen, cx| {
                                        screen.show_newest(database, window, cx)
                                    });
                                },
                                cx,
                            );
                        } else {
                            model.toast(message, cx);
                        }
                    }
                });
                if outcome.is_ok() && this.download_error.is_none() {
                    this.import_dialog = false;
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Shows all games of a database, newest first, as after a download.
    pub(super) fn show_newest(
        &mut self,
        database: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_filter_fields(window, cx);
        self.selected = None;
        self.selected_db = Some(database.clone());
        self.model.update(cx, |model, cx| {
            model.config.database_sorts.insert(
                database.to_string_lossy().into_owned(),
                crate::config::SavedSort {
                    key: SortKey::Date.as_str().to_owned(),
                    descending: true,
                },
            );
            model.save_config(cx);
        });
        self.apply_filter(cx);
    }

    /// The running download inside the Import dialog: games so far, a 2px bar, and the
    /// site's requested wait.
    pub(super) fn render_download_progress(
        &self,
        running: &Downloading,
    ) -> impl IntoElement + use<> {
        let bar = match running.months {
            Some((done, total)) if total > 0 => {
                ui::widgets::progress_bar(done as f32 / total as f32).into_any_element()
            }
            _ => ui::widgets::busy_bar("download-dialog-bar").into_any_element(),
        };
        let status = running.status();
        v_flex()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(14.))
                    .child(
                        v_flex()
                            .child(div().text_as(10., T.faint).child(ui::i18n::t("GAMES")))
                            .child(mono(format_count(running.games)).text_as(12., T.fg_2)),
                    )
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_as(10., T.faint)
                                    .child(ui::i18n::t("ALREADY STORED")),
                            )
                            .child(mono(format_count(running.duplicates)).text_as(12., T.fg_2)),
                    ),
            )
            .child(bar)
            .child(div().text_as(13., T.fg).child(status))
            .children(running.months.map(|(done, total)| {
                div().text_as(13., T.muted).child(ui::i18n::tf(
                    "{} of {} months",
                    &[&done.to_string(), &total.to_string()],
                ))
            }))
    }

    /// "Downloading from Lichess: 412 games" with Cancel, while a download runs.
    pub(super) fn render_download_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let running = self.downloading.as_ref()?;
        let this = cx.entity();
        let text = format!("{}: {}", running.user, running.status());
        let counts = ui::i18n::tf(
            "{} new games, {} already stored",
            &[
                &format_count(running.games),
                &format_count(running.duplicates),
            ],
        );
        let bar = match running.months {
            Some((done, total)) if total > 0 => {
                ui::widgets::progress_bar(done as f32 / total as f32).into_any_element()
            }
            _ => ui::widgets::busy_bar("download-bar").into_any_element(),
        };
        Some(
            panel().flex_none().overflow_hidden().child(
                v_flex()
                    .child(
                        h_flex()
                            .justify_between()
                            .px(px(16.))
                            .py(px(8.))
                            .child(
                                v_flex()
                                    .gap(px(4.))
                                    .child(div().text_size(px(14.)).child(text))
                                    .child(mono(counts).text_as(12., T.fg_2)),
                            )
                            .child(
                                btn("cancel-download", ui::i18n::t("Cancel"))
                                    .compact()
                                    .disabled(running.control.is_cancelled())
                                    .on_click(this.listen(move |screen, _, cx| {
                                        if let Some(running) = &screen.downloading {
                                            running.control.cancel();
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(bar),
            ),
        )
    }
}
