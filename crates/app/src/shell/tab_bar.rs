//! The tab bar and the new tab picker.

use super::*;

#[derive(Clone)]
pub(super) struct DraggedTab {
    id: u64,
    title: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(rgb(T.selected))
            .child(self.title.clone())
    }
}

impl Workspace {
    /// Covers the content area when the + button is open: one card per kind of tab.
    pub(super) fn new_tab_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let choices: [(Mode, &'static str, &str, &str); 6] = [
            (
                Mode::Analyse,
                "icons/microscope.svg",
                ui::i18n::t("Analyse"),
                ui::i18n::t(
                    "A board, engine analysis and the move list for a new position or game.",
                ),
            ),
            (
                Mode::Play,
                "icons/swords.svg",
                ui::i18n::t("Play"),
                ui::i18n::t("Play a game against an engine, with clocks."),
            ),
            (
                Mode::Database,
                "icons/database.svg",
                ui::i18n::t("Database"),
                ui::i18n::t("Browse, search and import your game collections."),
            ),
            (
                Mode::Train,
                "icons/graduation-cap.svg",
                ui::i18n::t("Train"),
                ui::i18n::t("Drill your repertoire with spaced repetition."),
            ),
            (
                Mode::Tournament,
                "icons/trophy.svg",
                ui::i18n::t("Tournament"),
                ui::i18n::t("Engine-vs-engine matches run with fastchess."),
            ),
            (
                Mode::TestPositions,
                "icons/flask-conical.svg",
                ui::i18n::t("Test positions"),
                ui::i18n::t("Run engines over an EPD suite to find best moves or mates."),
            ),
        ];
        // Two per row, in one column with the header so their edges line up.
        let mut cards = div().grid().grid_cols(2).gap(px(16.)).w_full();
        for (index, (mode, icon_path, title, description)) in choices.into_iter().enumerate() {
            let pick = cx.entity();
            cards = cards.child(
                v_flex()
                    .id(("new-tab-card", index))
                    .min_w_0()
                    .min_h(px(132.))
                    .p(px(24.))
                    .gap(px(10.))
                    .rounded(px(12.))
                    .bg(rgb(T.surface))
                    .border_as(T.border)
                    .cursor_pointer()
                    .hover(|card| card.border_color(rgb(T.border_strong)).bg(rgb(T.selected)))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(ui::widgets::icon(icon_path, px(20.), T.accent))
                            .child(
                                div()
                                    .text_as(19., T.fg)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .text_as(14., T.muted)
                            .line_height(px(20.))
                            .child(description),
                    )
                    .on_click(pick.listen_event(move |this, _, window, cx| {
                        this.new_tab_menu_open = false;
                        this.new_tab(mode, window, cx)
                    })),
            );
        }
        let close = cx.entity();
        v_flex()
            .id("new-tab-picker")
            .absolute()
            .inset_0()
            .occlude()
            .items_center()
            .justify_center()
            .p(px(16.))
            .bg(rgb(T.bg))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(820.))
                    .gap(px(20.))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_as(22., T.fg)
                                    .font_weight(FontWeight::BOLD)
                                    .child(ui::i18n::t("New tab")),
                            )
                            .child(
                                btn("new-tab-picker-close", "×")
                                    .h(28.)
                                    .w(28.)
                                    .pad(4.)
                                    .tooltip(ui::i18n::t("Close"))
                                    .on_click(close.listen(move |this, _, cx| {
                                        this.new_tab_menu_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(cards)
                    .child({
                        let random = cx.entity();
                        let create = cx.entity();
                        h_flex()
                            .gap(px(8.))
                            .child(
                                div().w(px(80.)).child(
                                    crate::controls::text_input(&self.chess960_number).mono(),
                                ),
                            )
                            .child(
                                btn("new-960-random", "Random")
                                    .icon("icons/dices.svg")
                                    .compact()
                                    .on_click(random.listen(move |this, window, cx| {
                                        let n = random_chess960_number();
                                        this.chess960_number.update(cx, |input, cx| {
                                            input.set_value(n.to_string(), window, cx)
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(ui::widgets::mono(
                                self.chess960_number
                                    .read(cx)
                                    .value()
                                    .trim()
                                    .parse::<u16>()
                                    .ok()
                                    .filter(|n| *n < 960)
                                    .map_or_else(
                                        || ui::i18n::t("Enter a number from 0 to 959").to_owned(),
                                        |n| format!("#{n} {}", chess_core::chess960::back_rank(n)),
                                    ),
                            ))
                            .child(btn("new-960", "New Chess960 game…").compact().on_click(
                                move |window, cx| {
                                    create.update(cx, |this, cx| {
                                        let Some(n) = this
                                            .chess960_number
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .parse::<u16>()
                                            .ok()
                                            .filter(|n| *n < 960)
                                        else {
                                            this.model.toast("Enter a number from 0 to 959", cx);
                                            return;
                                        };
                                        this.new_tab_menu_open = false;
                                        this.new_chess960(n, window, cx);
                                    });
                                },
                            ))
                    }),
            )
    }

    /// Opens a new Analyse tab on Chess960 start position `n` and remembers the number.
    pub(super) fn new_chess960(&mut self, n: u16, window: &mut Window, cx: &mut Context<Self>) {
        self.model
            .edit_config(cx, |config| config.chess960_number = n);
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse.update(cx, |screen, cx| {
            screen.load_tree(chess_core::GameTree::chess960(n), window, cx)
        });
    }

    pub(super) fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // A new or newly chosen tab scrolls into view.
        if self.tab_in_view.get() != self.active_tab
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == self.active_tab)
        {
            self.tab_in_view.set(self.active_tab);
            self.tab_scroll.scroll_to_item(index);
        }
        let scroll = self.tab_scroll.clone();
        let mut tabs = h_flex()
            .id("workspace-tabs")
            .min_w_0()
            .h_full()
            .items_end()
            .gap(px(2.))
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            // A plain mouse wheel scrolls up and down; in the strip it moves along the tabs.
            .on_scroll(move |event: &ScrollEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                let step = if delta.x.abs() > delta.y.abs() {
                    delta.x
                } else {
                    delta.y
                };
                let max = scroll.max_offset().x;
                if max <= px(0.) || step == px(0.) {
                    return;
                }
                let mut offset = scroll.offset();
                offset.x = (offset.x + step).clamp(-max, px(0.));
                scroll.set_offset(offset);
                cx.stop_propagation();
                window.refresh();
            });
        for tab in &self.tabs {
            tabs = tabs.child(self.render_tab(tab, cx));
        }
        h_flex()
            .relative()
            .flex_none()
            .h(px(38.))
            .px(px(10.))
            .gap(px(6.))
            .items_end()
            .bg(rgb(T.surface))
            // The bottom rule is drawn first so the open tab, painted over it, can hide it.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(rgb(T.border)),
            )
            // The strip only takes the width its tabs need, so New tab sits right after the
            // last one; with many tabs the strip scrolls and the button stays in view.
            .child(tabs)
            .child(self.new_tab_button(cx))
    }

    /// One tab: its label or rename field, its close button and context menu. Tabs can be
    /// dragged to reorder them, and a middle click closes one.
    fn render_tab(&self, tab: &WorkspaceTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = tab.id;
        let select = cx.entity();
        let close = select.clone();
        let duplicate = select.clone();
        let rename = select.clone();
        let close_all = select.clone();
        let analysis = matches!(tab.content, TabContent::Analyse(_));
        let active = id == self.active_tab;
        let rename_input = self
            .renaming
            .as_ref()
            .filter(|rename| rename.key == id)
            .map(|rename| rename.input.clone());
        let label: SharedString = tab
            .name
            .clone()
            .unwrap_or_else(|| tab.content.title(cx).0.into());
        h_flex()
            .id(("tab", id as usize))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.close_tab(id, window, cx);
                }),
            )
            .on_drag(
                DraggedTab {
                    id,
                    title: label.clone(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .drag_over::<DraggedTab>(|style, _, _, _| style.border_color(rgb(T.accent)))
            .on_drop(cx.listener(move |this, drag: &DraggedTab, _, cx| {
                let source = this.tabs.iter().position(|tab| tab.id == drag.id);
                let target = this.tabs.iter().position(|tab| tab.id == id);
                if let (Some(source), Some(target)) = (source, target) {
                    let tab = this.tabs.remove(source);
                    this.tabs.insert(target, tab);
                    this.save_session(cx);
                    cx.notify();
                }
            }))
            .flex_none()
            .gap(px(2.))
            .h(px(33.))
            .pl(px(11.))
            .pr(px(4.))
            .rounded_t(px(6.))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            // The open tab takes the screen's background and has no bottom edge, so it
            // runs straight into the screen below it.
            .map(|tab| {
                if active {
                    tab.bg(rgb(T.bg)).border_color(rgb(T.border))
                } else {
                    tab.border_color(gpui::transparent_black())
                        .hover(|tab| tab.bg(rgb(T.hover)))
                }
            })
            .when_some(rename_input, |tab, input| {
                let cancel = cx.entity();
                tab.child(
                    div()
                        .w(px(180.))
                        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                cancel.update(cx, |this, cx| this.finish_rename(false, window, cx));
                            }
                        })
                        .child(crate::controls::text_input(&input).height(26.)),
                )
            })
            .when(
                self.renaming.as_ref().is_none_or(|rename| rename.key != id),
                |el| el.child(self.tab_label(tab, cx)),
            )
            .child(
                btn(("close-tab", id as usize), "×")
                    .kind(ui::widgets::BtnKind::Ghost)
                    .h(24.)
                    .w(24.)
                    .pad(3.)
                    .tooltip(ui::i18n::t("Close tab; reopen with Ctrl/Cmd+Shift+T"))
                    .on_click(close.listen(move |this, window, cx| this.close_tab(id, window, cx))),
            )
            .context_menu(move |menu, _, _| {
                let rename = rename.clone();
                let menu = menu.item(PopupMenuItem::new(ui::i18n::t("Rename tab")).on_click(
                    move |_, window, cx| {
                        rename.update(cx, |this, cx| this.start_rename(id, window, cx))
                    },
                ));
                let close_all = close_all.clone();
                let menu = menu.item(PopupMenuItem::new(ui::i18n::t("Close all tabs")).on_click(
                    move |_, window, cx| {
                        close_all.update(cx, |this, cx| this.close_all_tabs(window, cx));
                    },
                ));
                if !analysis {
                    return menu;
                }
                let duplicate = duplicate.clone();
                menu.item(PopupMenuItem::new("Duplicate analysis tab").on_click(
                    move |_, window, cx| {
                        duplicate.update(cx, |this, cx| {
                            let Some(TabContent::Analyse(source)) = this
                                .tabs
                                .iter()
                                .find(|tab| tab.id == id)
                                .map(|tab| tab.content.clone())
                            else {
                                return;
                            };
                            let tree = source.read(cx).tree.clone();
                            let current = source.read(cx).current;
                            this.open_analysis(tree, window, cx);
                            this.analyse
                                .update(cx, |screen, cx| screen.jump(current, window, cx));
                        });
                    },
                ))
            })
    }

    /// A tab's number, name and what it is showing, with its engines' activity. Clicking
    /// selects the tab and a double click renames it.
    fn tab_label(&self, tab: &WorkspaceTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = tab.id;
        let select = cx.entity();
        let active = id == self.active_tab;
        let (screen, detail) = tab.content.title(cx);
        let label: SharedString = tab.name.clone().unwrap_or_else(|| screen.into());
        let detail = if tab.name.is_some() { None } else { detail };
        // The shown tab's own screen already says what its engines are doing.
        let activity = tab
            .content
            .activity(cx)
            .filter(|activity| !active || *activity == TabActivity::Paused);
        div()
            .id(("tab-select", id as usize))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(rgb(if active { T.fg } else { T.muted }))
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new(ui::i18n::t("Double-click to rename"))
                    .build(window, cx)
            })
            .on_click(
                select.listen_event(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.activate_tab(id, cx);
                    if event.click_count() == 2 {
                        this.start_rename(id, window, cx);
                    }
                }),
            )
            .child({
                h_flex()
                    .gap(px(6.))
                    .child(ui::widgets::mono(id.to_string()).text_color(rgb(T.faint)))
                    .child(
                        div()
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .child(label),
                    )
                    .children(detail.map(|detail| {
                        div()
                            .when(active, |d| d.text_color(rgb(T.muted)))
                            .child(detail)
                    }))
                    .children(activity.map(TabActivity::mark))
            })
    }

    /// New tab, drawn as one more tab: closed it looks like the inactive tabs, and while
    /// its picker is open it looks like the open tab.
    fn new_tab_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let add = cx.entity();
        let tip = match self.model.read(cx).config.shortcuts.get("new_tab") {
            Some(key) if !key.is_empty() => {
                format!(
                    "{} ({})",
                    ui::i18n::t("Open a new tab"),
                    crate::screens::settings::key_names(key).join("+")
                )
            }
            _ => ui::i18n::t("Open a new tab").to_owned(),
        };
        let open = self.new_tab_menu_open;
        h_flex()
            .id("new-tab")
            .flex_none()
            .h(px(33.))
            .px(px(11.))
            .gap(px(6.))
            .rounded_t(px(6.))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .cursor_pointer()
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .map(|tab| {
                if open {
                    tab.bg(rgb(T.bg))
                        .border_color(rgb(T.border))
                        .text_color(rgb(T.fg))
                } else {
                    tab.border_color(gpui::transparent_black())
                        .text_color(rgb(T.muted))
                        .hover(|tab| tab.bg(rgb(T.hover)).text_color(rgb(T.fg)))
                }
            })
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .on_click(add.listen_event(move |this, _, _, cx| {
                this.new_tab_menu_open = !this.new_tab_menu_open;
                cx.notify();
            }))
            .child(ui::widgets::icon(
                "icons/plus.svg",
                px(14.),
                if open { T.fg } else { T.muted },
            ))
            .child(ui::i18n::t("New tab"))
    }
}
