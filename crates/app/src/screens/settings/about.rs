//! Settings › General: updates, opening data, licences, thanks and the changelog.

use super::*;

/// A day as YYYY-MM-DD from seconds since 1970 (UTC).
pub fn date_of(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The Changelog page: `CHANGELOG.md`, one panel per release with its changes listed.
pub(super) fn render_changelog() -> Div {
    let mut page = v_flex()
        .gap(px(16.))
        .child(page_title(ui::i18n::t("Changelog")));
    let mut release: Option<Div> = None;
    for line in include_str!("../../../../../CHANGELOG.md").lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            page = page.children(release.take());
            release = Some(section(heading.trim()));
        } else if let Some(entry) = line.strip_prefix("- ")
            && let Some(current) = release.take()
        {
            release = Some(
                current.child(
                    div()
                        .py(px(6.))
                        .border_b_1()
                        .border_color(rgb(T.divider))
                        .text_as(13., T.fg_2)
                        .child(entry.trim()),
                ),
            );
        }
    }
    page.children(release)
}

/// The bundled assets (piece sets, sounds, fonts) with their licences and authors.
fn bundled_assets_table() -> gpui::Div {
    use crate::licenses::ASSETS;
    let asset_name = gpui::relative(0.36);
    let asset_license = gpui::relative(0.24);
    let assets = panel().overflow_hidden().child(
        h_flex()
            .w_full()
            .h(px(30.))
            .px(px(12.))
            .bg(rgb(T.surface_2))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(
                div()
                    .w(asset_name)
                    .flex_shrink_0()
                    .child(section_label("Asset")),
            )
            .child(
                div()
                    .w(asset_license)
                    .flex_shrink_0()
                    .child(section_label("Licence")),
            )
            .child(section_label("By")),
    );
    let mut asset_rows = v_flex().w_full();
    for (name, author, license) in ASSETS {
        asset_rows = asset_rows.child(
            h_flex()
                .w_full()
                .h(px(34.))
                .flex_shrink_0()
                .px(px(12.))
                .border_b_1()
                .border_color(rgb(T.divider))
                .text_size(px(13.))
                .child(
                    div()
                        .w(asset_name)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                )
                .child(
                    mono(license)
                        .w(asset_license)
                        .flex_shrink_0()
                        .text_as(12., T.fg_2),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(rgb(T.muted))
                        .child(author),
                ),
        );
    }
    assets.child(
        div()
            .id("bundled-assets-rows")
            .w_full()
            .max_h(px(204.))
            .child(asset_rows)
            .overflow_y_scrollbar(),
    )
}

impl SettingsScreen {
    /// The General page: updates, opening data, theme and language. Board, shortcut and
    /// storage settings live on their own pages only.
    pub(super) fn render_general(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("General")))
            .child(self.render_appearance(cx))
            .child(self.render_app_update(cx))
            .child(self.render_opening_data(cx))
            .when(
                {
                    let model = self.model.read(cx);
                    model.unsaved_config || model.unsaved_repertoire
                },
                |page| {
                    page.child(
                        btn("retry-saves", "Retry saving settings and repertoire").on_click({
                            let model = self.model.clone();
                            move |_, cx| {
                                model.update(cx, |model, cx| {
                                    model.save_config(cx);
                                    model.save_repertoire(cx);
                                })
                            }
                        }),
                    )
                },
            )
    }

    /// Show the installed version and user-requested application update controls.
    pub(super) fn render_app_update(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let busy = self.app_update_task.is_some();
        let buttons = h_flex()
            .gap(px(8.))
            .child(
                btn("check-fast-update", "Check for updates")
                    .compact()
                    .disabled(busy)
                    .on_click(this.listen(move |screen, _, cx| screen.check_app_update(cx))),
            )
            .when(self.app_update.is_some(), |row| {
                row.child(
                    btn("install-fast-update", "Download and install update")
                        .compact()
                        .primary()
                        .disabled(busy)
                        .on_click(this.listen(move |screen, _, cx| screen.install_app_update(cx))),
                )
            });
        let available = self.app_update.as_ref().map(|update| {
            h_flex()
                .gap(px(8.))
                .child(ui::i18n::t("Available version"))
                .child(mono(update.version.clone()))
        });
        panel().p(px(16.)).child(
            v_flex()
                .gap(px(8.))
                .child(section_label("Fast updates"))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(ui::i18n::t("Installed version"))
                        .child(mono(crate::app_update::VERSION)),
                )
                .child(ui::i18n::t(
                    "Check for a new Fast release. Updates keep your settings, databases and engines.",
                ))
                .child(buttons)
                .children(available)
                .children(
                    self.app_update_status
                        .clone()
                        .map(|status| div().text_color(rgb(T.fg_2)).child(status)),
                ),
        )
    }

    /// Check public releases in the background without disturbing the current session.
    pub(super) fn check_app_update(&mut self, cx: &mut Context<Self>) {
        if self.app_update_task.is_some() {
            return;
        }
        self.app_update = None;
        self.app_update_status = Some(ui::i18n::t("Checking for updates…").into());
        let worker = cx.background_spawn(async { crate::app_update::check() });
        self.app_update_task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.app_update_task = None;
                screen.app_update_status = Some(match result {
                    Ok(Some(update)) => {
                        screen.app_update = Some(update);
                        ui::i18n::t("A new version is available.").into()
                    }
                    Ok(None) => ui::i18n::t("Fast is up to date.").into(),
                    Err(error) => format!(
                        "{} {error:#}",
                        ui::i18n::t("Could not check for updates. Try again later.")
                    )
                    .into(),
                });
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Verify a release download and install it through the current platform's update flow.
    pub(super) fn install_app_update(&mut self, cx: &mut Context<Self>) {
        if self.app_update_task.is_some() {
            return;
        }
        let Some(update) = self.app_update.clone() else {
            return;
        };
        self.app_update_status = Some(ui::i18n::t("Downloading and verifying update…").into());
        let worker = cx.background_spawn(async move { crate::app_update::install(&update) });
        self.app_update_task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.app_update_task = None;
                screen.app_update_status = Some(match result {
                    Ok(replaced) => {
                        screen.app_update = None;
                        ui::i18n::t(if replaced {
                            "Update installed. Close and reopen Fast to use the new version."
                        } else {
                            "Installer opened. Finish installation, then close and reopen Fast."
                        })
                        .into()
                    }
                    Err(error) => {
                        format!("{} {error:#}", ui::i18n::t("Update failed. You can retry.")).into()
                    }
                });
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render_opening_data(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let updating = self.opening_task.is_some();
        panel().p(px(16.)).child(
            v_flex()
                .gap(px(8.))
                .child(section_label("Opening data"))
                .child(ui::i18n::t(
                    "Update opening names and ECO codes from Lichess. Updates apply immediately \
                     and are saved for offline use. Bundled data is the fallback.",
                ))
                .child(
                    btn(
                        "update-opening-data",
                        if updating {
                            "Updating…"
                        } else {
                            "Update opening data"
                        },
                    )
                    .disabled(updating)
                    .on_click(this.listen(move |screen, _, cx| screen.update_opening_data(cx))),
                )
                .children(self.opening_status.clone()),
        )
    }

    pub(super) fn update_opening_data(&mut self, cx: &mut Context<Self>) {
        if self.opening_task.is_some() {
            return;
        }
        self.opening_status = Some("Downloading Lichess opening data…".into());
        let data = self.model.read(cx).paths.data_dir.clone();
        let worker = cx.background_spawn(async move { crate::opening_data::update(&data) });
        self.opening_task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.opening_task = None;
                screen.opening_status = Some(match result {
                    Ok(message) => message,
                    Err(err) => format!("Opening update failed: {err:#}. Existing opening data is still active. You can retry."),
                }.into());
                screen.model.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Fast's licence, the bundled assets and every library, filterable.
    pub(super) fn render_licenses(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use crate::licenses::{FAST_LICENSE, libraries, license_counts};
        let all = libraries();
        let query = self.license_filter.read(cx).value().to_string();
        let shown: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, library)| library.matches(&query))
            .map(|(index, _)| index)
            .collect();
        let col_name = gpui::relative(0.36);
        let col_version = gpui::relative(0.14);
        let col_license = gpui::relative(0.38);
        let header = |cells: [&'static str; 4]| {
            h_flex()
                .w_full()
                .h(px(30.))
                .px(px(12.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(
                    div()
                        .w(col_name)
                        .flex_shrink_0()
                        .child(section_label(cells[0])),
                )
                .child(
                    div()
                        .w(col_version)
                        .flex_shrink_0()
                        .child(section_label(cells[1])),
                )
                .child(
                    div()
                        .w(col_license)
                        .flex_shrink_0()
                        .child(section_label(cells[2])),
                )
                .child(section_label(cells[3]))
        };
        let assets = bundled_assets_table();
        let summary = license_counts(all)
            .into_iter()
            .take(6)
            .map(|(license, count)| format!("{count} {license}"))
            .collect::<Vec<_>>()
            .join(", ");
        let count = shown.len();
        let list = uniform_list(
            "license-rows",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                range
                    .filter_map(|row| {
                        let library = &all[*shown.get(row)?];
                        let repository = library.repository.clone();
                        Some(
                            h_flex()
                                .id(("library", row))
                                .w_full()
                                .h(px(32.))
                                .px(px(12.))
                                .border_b_1()
                                .border_color(rgb(T.divider))
                                .text_size(px(13.))
                                .child(
                                    div()
                                        .w(col_name)
                                        .flex_shrink_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(library.name.clone()),
                                )
                                .child(
                                    mono(library.version.clone())
                                        .w(col_version)
                                        .flex_shrink_0()
                                        .text_as(12., T.muted),
                                )
                                .child(
                                    mono(library.license.clone())
                                        .w(col_license)
                                        .flex_shrink_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_as(12., T.fg_2),
                                )
                                .children(repository.map(|url| {
                                    div()
                                        .id(("library-source", row))
                                        .cursor_pointer()
                                        .text_color(rgb(T.accent))
                                        .hover(|style| style.underline())
                                        .on_click(move |_, _, cx| cx.open_url(&url))
                                        .child(ui::i18n::t("Source"))
                                })),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.license_scroll)
        .h(px(460.));
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Licenses")))
            .child(
                section("Fast").child(
                    div()
                        .text_as(14., T.fg_2)
                        .child(format!(
                            "Fast is free software under {FAST_LICENSE}. The libraries and assets below keep their own licences."
                        )),
                ),
            )
            .child(section(ui::i18n::t("Bundled assets")).child(assets))
            .child(
                section(ui::i18n::t("Libraries"))
                    .child(
                        div()
                            .mb(px(10.))
                            .text_as(13., T.muted)
                            .child(format!(
                                "{} Rust crates from Cargo.lock, including ones only built on other platforms. Most common: {summary}.",
                                all.len()
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .mb(px(10.))
                            .child(div().w(px(320.)).child(text_input(&self.license_filter)))
                            .child(
                                div()
                                    .text_as(13., T.muted)
                                    .child(format!("{count} shown")),
                            ),
                    )
                    .child(
                        panel()
                            .overflow_hidden()
                            .child(header(["Crate", "Version", "Licence", "Source"]))
                            .child(
                                div()
                                    .relative()
                                    .child(list)
                                    .vertical_scrollbar(&self.license_scroll),
                            ),
                    ),
            )
    }

    /// Acknowledge the chess projects whose work and community help guide Fast.
    pub(super) fn render_thanks(&self) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Thanks")))
            .child(
                section("En Croissant")
                    .child(div().text_color(rgb(T.fg_2)).child(ui::i18n::t(
                        "Thanks to En Croissant and its contributors for a carefully handcrafted chess GUI with a rich set of features. Its project history and community requests were valuable resources for the implementation of this GUI.",
                    )))
                    .child(
                        h_flex().mt(px(12.)).child(
                            btn("en-croissant-project", "Visit En Croissant")
                                .compact()
                                .on_click(|_, cx| {
                                    cx.open_url("https://github.com/franciscoBSalgueiro/en-croissant");
                                }),
                        ),
                    ),
            )
    }
}
