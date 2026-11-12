//! Settings › Databases: the opening explorer's databases.

use super::*;

impl SettingsScreen {
    /// Removes a reference database from the list while keeping its file recoverable.
    pub(super) fn remove_database(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, cx| model.remove_database(path, cx));
    }

    /// Shows player names, accounts and the locally stored databases.
    pub(super) fn render_databases(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let mut rows = v_flex();
        for (index, entry) in model.databases.iter().enumerate() {
            let this = cx.entity();
            let path = entry.path.clone();
            let removable = path != model.my_games_path();
            rows = rows.child(
                h_flex()
                    .h(px(43.))
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap(px(16.))
                            .child(mono(format_count(entry.games)).text_as(14., T.muted))
                            .child(
                                btn(("remove-database", index), "Remove")
                                    .compact()
                                    .disabled(!removable)
                                    .tooltip(ui::i18n::t("Move to the removed folder"))
                                    .on_click(move |_, cx| {
                                        this.update(cx, |screen, cx| {
                                            screen.remove_database(path.clone(), cx)
                                        });
                                    }),
                            ),
                    ),
            );
        }
        if model.databases.is_empty() {
            rows = rows.child(
                div()
                    .py(px(10.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(
                        "No databases yet. Import a PGN from the Database screen.",
                    )),
            );
        }
        let location = model.paths.databases_dir().display().to_string();
        let mut accounts = v_flex();
        for (index, account) in model.config.accounts.iter().enumerate() {
            let database = model
                .databases
                .iter()
                .find(|entry| entry.path == account.database)
                .map_or_else(|| SharedString::from("—"), |entry| entry.name.clone());
            let updated = account
                .updated_at
                .map_or_else(|| "—".to_owned(), crate::screens::settings::date_of);
            let (update, forget) = (self.workspace.clone(), self.model.clone());
            accounts = accounts.child(
                h_flex()
                    .h(px(43.))
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .child(
                        div()
                            .w(px(84.))
                            .flex_none()
                            .text_as(13., T.muted)
                            .child(account.site.name()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(account.name.clone()),
                    )
                    .child(
                        div()
                            .w(px(140.))
                            .flex_none()
                            .truncate()
                            .text_as(13., T.fg_2)
                            .child(database),
                    )
                    .child(
                        mono(updated)
                            .w(px(96.))
                            .flex_none()
                            .whitespace_nowrap()
                            .text_as(13., T.fg_2),
                    )
                    .child(btn(("account-update", index), "Update").compact().on_click(
                        move |_, cx| {
                            if let Some(workspace) = update.upgrade() {
                                workspace.update(cx, |workspace, cx| {
                                    workspace.update_account(index, cx)
                                });
                            }
                        },
                    ))
                    .child(btn(("account-forget", index), "Forget").compact().on_click(
                        move |_, cx| {
                            forget.update(cx, |model, cx| {
                                if index < model.config.accounts.len() {
                                    model.config.accounts.remove(index);
                                    model.save_config(cx);
                                }
                            });
                        },
                    )),
            );
        }
        if model.config.accounts.is_empty() {
            accounts = accounts.child(div().py(px(10.)).text_as(13., T.muted).child(ui::i18n::t(
                "No accounts yet. Use Import › Lichess or Chess.com on the Database screen.",
            )));
        }
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Databases")))
            .child(section(ui::i18n::t("You")).child(option_row(
                "Player name",
                Some(ui::i18n::t(
                    "How you appear in PGN headers. Used for My games and leak detection.",
                )),
                div().w(px(320.)).child(text_input(&self.player_name)),
            )).child(option_row(
                "Player aliases",
                Some(ui::i18n::t("Other names for My games and leak detection. Separate names with semicolons.")),
                div().w(px(320.)).child(text_input(&self.player_aliases)),
            )))
            .child(
                section(ui::i18n::t("Accounts"))
                    .child(accounts)
                    .child(
                        div()
                            .mt(px(10.))
                            .text_as(13., T.muted)
                            .child(ui::i18n::t(
                                "Updates only run when you ask. Each one fetches the games played since the last.",
                            )),
                    ),
            )
            .child(
                section(ui::i18n::t("Files")).child(rows).child(
                    div().mt(px(10.)).text_as(13., T.muted)
                        .child(ui::i18n::t("Removed databases are kept in the removed folder.")),
                ).child(
                    div()
                        .mt(px(10.))
                        .text_as(13., T.muted)
                        .child(format!("Stored in {location}")),
                ),
            )
    }
}
