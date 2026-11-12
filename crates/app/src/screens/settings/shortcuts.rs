//! Settings › Shortcuts: listing and reassigning keyboard shortcuts.

use super::*;

/// The rebindable actions with their labels, grouped as Settings shows them.
pub(super) const SHORTCUT_GROUPS: [(&str, &[(&str, &str)]); 5] = [
    (
        "Navigation",
        &[
            ("prev_move", "Previous move"),
            ("next_move", "Next move"),
            ("first_move", "Start of game"),
            ("last_move", "End of line"),
            ("flip_board", "Flip board"),
        ],
    ),
    (
        "Game",
        &[
            ("new_game", "New game"),
            ("new_chess960", "New Chess960 game"),
            ("open_game", "Open PGN or FEN"),
            ("save_game", "Save to My games"),
            ("save_pgn_as", "Save as PGN"),
            ("paste_game", "Paste FEN or PGN"),
            ("copy_fen", "Copy FEN"),
            ("copy_pgn", "Copy PGN"),
        ],
    ),
    (
        "Engine",
        &[
            ("toggle_engine", "Engine on / off"),
            ("pause_engine", "Pause / resume"),
            ("analyse_backward", "Analyse game backwards"),
            ("more_lines", "More lines (MultiPV)"),
            ("fewer_lines", "Fewer lines (MultiPV)"),
            ("engine_output", "Engine output window"),
        ],
    ),
    (
        "Analysis display",
        &[
            ("all_moves", "All moves"),
            ("quality_arrows", "Quality arrows"),
            ("sort_lines", "Sort by eval / depth"),
            ("next_explorer_db", "Next opening-tree database"),
            ("next_graph", "Next graph"),
            ("scoresheet_style", "Table / inline scoresheet"),
        ],
    ),
    (
        "Window",
        &[
            ("new_tab", "New tab"),
            ("close_tab", "Close tab"),
            ("reopen_tab", "Reopen closed tab"),
            ("next_tab", "Next tab"),
            ("prev_tab", "Previous tab"),
            ("next_tab_alt", "Next tab, second key"),
            ("prev_tab_alt", "Previous tab, second key"),
            ("tab_back", "Go back"),
            ("tab_forward", "Go forward"),
            ("search", "Search"),
            ("show_analyse", "Analyse"),
            ("show_database", "Database"),
            ("show_train", "Train"),
            ("show_play", "Play"),
            ("show_settings", "Settings"),
            ("quit", "Quit"),
        ],
    ),
];

/// The label of a rebindable action.
pub(super) fn shortcut_label(action: &str) -> &'static str {
    SHORTCUT_GROUPS
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .find(|(name, _)| *name == action)
        .map_or("another action", |(_, label)| label)
}

/// The keys of a shortcut such as "ctrl-k", with friendly names ("Ctrl", "K").
pub(crate) fn key_names(keystroke: &str) -> Vec<String> {
    keystroke
        .split('-')
        .map(|part| match part {
            "left" => "←".to_owned(),
            "right" => "→".to_owned(),
            "up" => "↑".to_owned(),
            "down" => "↓".to_owned(),
            "ctrl" => "Ctrl".to_owned(),
            "cmd" => "⌘".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
        })
        .collect()
}

/// Renders "ctrl-k" as separate caps, with friendly names.
pub(super) fn keycaps(keystroke: Option<&String>) -> Div {
    let mut row = h_flex().gap(px(4.));
    let Some(keystroke) = keystroke.filter(|keystroke| !keystroke.is_empty()) else {
        return row;
    };
    for text in key_names(keystroke) {
        row = row.child(key_cap(text));
    }
    row
}

/// A shortcut as one small cap ("Ctrl K"), for hints inside other controls.
pub(crate) fn shortcut_hint(keystroke: &str) -> Div {
    key_cap(key_names(keystroke).join(" "))
        .h(px(20.))
        .px(px(5.))
        .text_as(11., T.fg_2)
}

impl SettingsScreen {
    pub(super) fn on_capture_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(action) = self.capturing else { return };
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.capturing = None;
            cx.notify();
            return;
        }
        // Modifiers alone do not make a shortcut.
        if matches!(
            keystroke.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        ) {
            return;
        }
        let mut text = String::new();
        let modifiers = keystroke.modifiers;
        for (active, name) in [
            (modifiers.control, "ctrl-"),
            (modifiers.alt, "alt-"),
            (modifiers.shift, "shift-"),
            (modifiers.platform, "cmd-"),
        ] {
            if active {
                text.push_str(name);
            }
        }
        text.push_str(&keystroke.key);
        cx.stop_propagation();
        self.assign_shortcut(action, text, cx);
        self.capturing = None;
        cx.notify();
    }

    /// Binds `keystroke` to `action` (empty clears it). Another action holding the
    /// same keystroke loses it, so no key ever triggers two actions.
    pub(super) fn assign_shortcut(
        &mut self,
        action: &str,
        keystroke: String,
        cx: &mut Context<Self>,
    ) {
        self.model.update(cx, |model, cx| {
            let freed: Vec<String> = if keystroke.is_empty() {
                Vec::new()
            } else {
                model
                    .config
                    .shortcuts
                    .iter()
                    .filter(|(other, key)| other.as_str() != action && **key == keystroke)
                    .map(|(other, _)| other.clone())
                    .collect()
            };
            for other in &freed {
                if let Some(old) = model.config.shortcuts.get(other).cloned() {
                    shell::unbind_shortcut(&old, other, cx);
                }
                model.config.shortcuts.insert(other.clone(), String::new());
            }
            if let Some(old) = model.config.shortcuts.get(action).cloned() {
                shell::unbind_shortcut(&old, action, cx);
            }
            model
                .config
                .shortcuts
                .insert(action.to_owned(), keystroke.clone());
            let single: std::collections::BTreeMap<String, String> =
                [(action.to_owned(), keystroke.clone())]
                    .into_iter()
                    .collect();
            shell::bind_shortcuts(&single, cx);
            model.save_config(cx);
            if let Some(other) = freed.first() {
                model.toast(
                    format!(
                        "{keystroke} now does {}; {} has no shortcut.",
                        shortcut_label(action),
                        shortcut_label(other)
                    ),
                    cx,
                );
            }
        });
        cx.notify();
    }

    pub(super) fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        let defaults = crate::config::default_shortcuts();
        self.model.update(cx, |model, cx| {
            for (action, key) in &model.config.shortcuts {
                shell::unbind_shortcut(key, action, cx);
            }
            model.config.shortcuts = defaults.clone();
            shell::bind_shortcuts(&defaults, cx);
            model.save_config(cx);
            model.toast("Shortcuts reset to their defaults.", cx);
        });
        cx.notify();
    }

    pub(super) fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let model = self.model.read(cx);
        let defaults = crate::config::default_shortcuts();
        let mut page = v_flex().gap(px(16.)).child(
            h_flex()
                .justify_between()
                .child(page_title(ui::i18n::t("Keyboard shortcuts")))
                .child(btn("reset-shortcuts", ui::i18n::t("Reset all")).on_click({
                    let this = this.clone();
                    move |_, cx| this.update(cx, |screen, cx| screen.reset_all_shortcuts(cx))
                })),
        );
        let mut index = 0usize;
        for (group, rows) in SHORTCUT_GROUPS {
            let mut list = v_flex();
            for &(action, label) in rows {
                index += 1;
                let capturing = self.capturing == Some(action);
                let current = model
                    .config
                    .shortcuts
                    .get(action)
                    .cloned()
                    .unwrap_or_default();
                let default = defaults.get(action).cloned().unwrap_or_default();
                let capture_focus = self.capture_focus.clone();
                let (change_this, clear_this, reset_this) =
                    (this.clone(), this.clone(), this.clone());
                list = list.child(
                    h_flex()
                        .h(px(40.))
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(T.divider))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(ui::i18n::t(label)),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(if capturing {
                                    div()
                                        .text_as(13., T.warn)
                                        .child(ui::i18n::t("Press a key… (Esc cancels)"))
                                } else if current.is_empty() {
                                    div().text_as(13., T.muted).child(ui::i18n::t("None"))
                                } else {
                                    keycaps(Some(&current))
                                })
                                .child(btn(("rebind", index), "Change").h(28.).pad(10.).on_click(
                                    move |window, cx| {
                                        capture_focus.focus(window, cx);
                                        change_this.update(cx, |screen, cx| {
                                            screen.capturing = Some(action);
                                            cx.notify();
                                        });
                                    },
                                ))
                                .child(
                                    btn(("clear-shortcut", index), "Clear")
                                        .h(28.)
                                        .pad(10.)
                                        .disabled(current.is_empty())
                                        .on_click(clear_this.listen(move |screen, _, cx| {
                                            screen.assign_shortcut(action, String::new(), cx);
                                        })),
                                )
                                .child(
                                    btn(("reset-shortcut", index), "Reset")
                                        .h(28.)
                                        .pad(10.)
                                        .disabled(current == default)
                                        .tooltip(if default.is_empty() {
                                            ui::i18n::t("No default").to_owned()
                                        } else {
                                            format!("Default: {default}")
                                        })
                                        .on_click(move |_, cx| {
                                            let default = default.clone();
                                            reset_this.update(cx, |screen, cx| {
                                                screen.assign_shortcut(action, default, cx);
                                            });
                                        }),
                                ),
                        ),
                );
            }
            page = page.child(section(group).child(list));
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_shortcut_is_listed_in_settings_once() {
        let listed: Vec<&str> = SHORTCUT_GROUPS
            .iter()
            .flat_map(|(_, rows)| rows.iter().map(|(action, _)| *action))
            .collect();
        let defaults = crate::config::default_shortcuts();
        assert_eq!(listed.len(), defaults.len());
        for action in defaults.keys() {
            assert_eq!(
                listed
                    .iter()
                    .filter(|name| **name == action.as_str())
                    .count(),
                1,
                "{action}"
            );
        }
        assert_eq!(shortcut_label("flip_board"), "Flip board");
    }
}
