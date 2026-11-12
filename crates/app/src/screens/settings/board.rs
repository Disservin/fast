//! Settings › Board: the theme, board colours, pieces and sounds.

use super::*;

pub(super) fn tile(id: impl Into<gpui::ElementId>, selected: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(8.))
        .w(px(74.))
        .rounded(px(6.))
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if selected { T.fg } else { T.border_strong }))
        .hover(|style| style.bg(rgb(T.hover)))
}

pub(super) fn tile_label(text: &'static str) -> Div {
    div().text_as(13., T.fg_2).child(ui::i18n::t(text))
}

pub(super) fn board_check(
    id: &'static str,
    value: bool,
    label: &'static str,
    this: Entity<SettingsScreen>,
    set: fn(&mut crate::config::BoardConfig, bool),
) -> impl IntoElement {
    div().child(
        check(id, value, label).on_change(this.listen_with(move |screen, value, _, cx| {
            screen.update_board(cx, |board| set(board, value));
        })),
    )
}

impl SettingsScreen {
    pub(super) fn update_board(
        &self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::config::BoardConfig),
    ) {
        self.model.update(cx, |model, cx| {
            change(&mut model.config.board);
            model.save_config(cx);
            cx.emit(ModelEvent::BoardPrefs);
        });
    }

    pub(super) fn render_appearance(&mut self, cx: &mut Context<Self>) -> Div {
        let selected = self.model.read(cx).config.ui_theme;
        let mut choices = h_flex().flex_wrap().gap(px(8.));
        for (index, choice) in ui::UiTheme::ALL.into_iter().enumerate() {
            let palette = choice.palette();
            choices = choices.child(
                tile(("ui-theme", index), selected == choice)
                    .child(tile_label(choice.name()))
                    .child(
                        h_flex().gap(px(3.)).children(
                            [palette.bg, palette.surface, palette.accent, palette.fg]
                                .into_iter()
                                .map(|color| div().size(px(10.)).rounded(px(2.)).bg(rgb(color))),
                        ),
                    )
                    .on_click(cx.listener(move |screen, _, _, cx| {
                        ui::set_theme(choice, cx);
                        screen.model.update(cx, |model, cx| {
                            model.config.set_ui_theme(choice);
                            model.save_config(cx);
                            cx.emit(ModelEvent::BoardPrefs);
                        });
                        cx.notify();
                    })),
            );
        }
        section(ui::i18n::t("Appearance"))
            .child(option_row(
                ui::i18n::t("Theme"),
                Some(ui::i18n::t("Applies to the entire interface")),
                choices,
            ))
            .child(option_row(
                ui::i18n::t("Language"),
                None,
                div().w(px(190.)).child(select_el(&self.language)),
            ))
    }

    pub(super) fn render_board_section(&mut self, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let config = self.model.read(cx).config.board.clone();
        let mut boards = h_flex().flex_wrap().gap(px(8.));
        let themes = [
            (SavedTheme::Blue, ui::BoardTheme::Blue),
            (SavedTheme::Walnut, ui::BoardTheme::Walnut),
            (SavedTheme::Moss, ui::BoardTheme::Moss),
            (SavedTheme::Graphite, ui::BoardTheme::Graphite),
            (SavedTheme::Paper, ui::BoardTheme::Paper),
            (SavedTheme::Plum, ui::BoardTheme::Plum),
        ];
        for (index, (saved, theme)) in themes.into_iter().enumerate() {
            let selected = config.theme == saved;
            boards = boards.child(
                tile(("board-theme", index), selected)
                    .on_click(this.listen_event(move |screen, _, _, cx| {
                        screen.update_board(cx, |board| board.theme = saved);
                    }))
                    .child(theme_swatch(theme, px(56.), cx))
                    .child(tile_label(theme.name())),
            );
        }
        let mut pieces = h_flex().flex_wrap().gap(px(8.));
        let knight = |set: ui::PieceSet| {
            img(set.asset(chess_core::shakmaty::Piece {
                color: chess_core::Color::Black,
                role: chess_core::Role::Knight,
            }))
            .size(px(44.))
            .into_any_element()
        };
        let sets: [(SavedPieces, &str, gpui::AnyElement); 7] = [
            (
                SavedPieces::Classic,
                "Classic",
                knight(ui::PieceSet::Classic),
            ),
            (
                SavedPieces::Cburnett,
                "Cburnett",
                knight(ui::PieceSet::Cburnett),
            ),
            (
                SavedPieces::Chessnut,
                "Chessnut",
                knight(ui::PieceSet::Chessnut),
            ),
            (
                SavedPieces::MpChess,
                "MPChess",
                knight(ui::PieceSet::MpChess),
            ),
            (
                SavedPieces::Fantasy,
                "Fantasy",
                knight(ui::PieceSet::Fantasy),
            ),
            (
                SavedPieces::Letters,
                "Letters",
                div()
                    .text_size(px(32.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0x14161A))
                    .child("N")
                    .into_any_element(),
            ),
            (
                SavedPieces::Flat,
                "Flat",
                svg()
                    .path("pieces/merida/bN.svg")
                    .size(px(34.))
                    .text_color(rgb(0x14161A))
                    .into_any_element(),
            ),
        ];
        for (index, (saved, name, preview)) in sets.into_iter().enumerate() {
            let selected = config.pieces == saved;
            pieces = pieces.child(
                tile(("piece-set", index), selected)
                    .on_click(this.listen_event(move |screen, _, _, cx| {
                        screen.update_board(cx, |board| board.pieces = saved);
                    }))
                    .child(
                        div()
                            .size(px(56.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .bg(rgb(config.prefs().theme.colors().0))
                            .child(preview),
                    )
                    .child(tile_label(name)),
            );
        }
        let coords_this = this.clone();
        let check_row = h_flex()
            .flex_wrap()
            .gap_x(px(24.))
            .gap_y(px(8.))
            .child(board_check(
                "legal",
                config.show_legal,
                ui::i18n::t("Show legal moves"),
                this.clone(),
                |b, v| b.show_legal = v,
            ))
            .child(board_check(
                "last",
                config.highlight_last,
                ui::i18n::t("Highlight last move"),
                this.clone(),
                |b, v| b.highlight_last = v,
            ))
            .child(board_check(
                "move-badges",
                config.show_move_badges,
                "Show move quality badges",
                this.clone(),
                |b, v| b.show_move_badges = v,
            ))
            .child(board_check(
                "queen",
                config.auto_queen,
                "Auto-promote to queen",
                this.clone(),
                |b, v| b.auto_queen = v,
            ))
            .child(board_check(
                "click-arrows",
                config.click_arrows,
                "Click an engine arrow to play it",
                this.clone(),
                |b, v| b.click_arrows = v,
            ))
            .child(board_check(
                "sounds",
                config.move_sounds,
                ui::i18n::t("Move sounds"),
                this.clone(),
                |b, v| b.move_sounds = v,
            ));
        let sound_this = this.clone();
        let preview_this = this.clone();
        let sound_choices = segmented(
            "sound-profile",
            crate::sound::SoundProfile::ALL
                .into_iter()
                .map(|profile| (profile, profile.name().into(), false))
                .collect(),
            config.sound_profile,
            move |profile, _, cx| {
                sound_this.update(cx, |screen, cx| {
                    screen.update_board(cx, |board| board.sound_profile = profile);
                    screen.model.read(cx).config.board.play_sample();
                })
            },
        )
        .flex_wrap()
        .gap_y(px(6.));
        section(ui::i18n::t("Board & pieces"))
            .child(option_row(ui::i18n::t("Board"), None, boards))
            .child(option_row(ui::i18n::t("Pieces"), None, pieces))
            .child(option_row(
                ui::i18n::t("Coordinates"),
                None,
                segmented(
                    "coords",
                    vec![
                        (
                            SavedCoordinates::OnSquares,
                            ui::i18n::t("On squares").into(),
                            false,
                        ),
                        (
                            SavedCoordinates::Outside,
                            ui::i18n::t("Outside").into(),
                            false,
                        ),
                        (SavedCoordinates::Off, ui::i18n::t("Off").into(), false),
                    ],
                    config.coordinates,
                    move |value, _, cx| {
                        coords_this.update(cx, |screen, cx| {
                            screen.update_board(cx, |board| board.coordinates = value);
                        });
                    },
                ),
            ))
            .child(option_row(
                ui::i18n::t("Move sound"),
                Some(ui::i18n::t(
                    "Choose a sound and preview it at your current volume",
                )),
                h_flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(sound_choices)
                    .child(
                        btn("preview-move-sound", ui::i18n::t("Preview"))
                            .disabled(!config.move_sounds || config.sound_volume == 0)
                            .on_click(move |_, cx| {
                                preview_this
                                    .read(cx)
                                    .model
                                    .read(cx)
                                    .config
                                    .board
                                    .play_sample();
                            }),
                    ),
            ))
            .child(option_row(
                ui::i18n::t("Sound volume"),
                Some(ui::i18n::t("For move sounds")),
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(160.))
                            .child(Slider::new(&self.volume).bg(rgb(T.accent))),
                    )
                    .child(
                        mono(if config.move_sounds {
                            format!("{}%", config.sound_volume)
                        } else {
                            ui::i18n::t("Off").to_owned()
                        })
                        .text_as(13., T.muted),
                    ),
            ))
            .child(option_row(
                ui::i18n::t("Move animation"),
                None,
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(160.))
                            .child(Slider::new(&self.animation).bg(rgb(T.accent))),
                    )
                    .child(mono(format!("{} ms", config.animation_ms)).text_as(13., T.muted)),
            ))
            .child(option_row(ui::i18n::t("Notation"), None, {
                let mut row = h_flex().gap(px(6.));
                for (index, (value, label)) in [
                    (Notation::San, "Nf3"),
                    (Notation::Figurine, "♘f3"),
                    (Notation::Long, "g1-f3"),
                ]
                .into_iter()
                .enumerate()
                {
                    row = row.child(
                        btn(("notation", index), label)
                            .mono()
                            .pad(10.)
                            .selected(config.notation == value)
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.update_board(cx, |board| board.notation = value);
                            })),
                    );
                }
                row
            }))
            .child(option_row(ui::i18n::t("Board behaviour"), None, check_row))
    }

    pub(super) fn render_board_page(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Board & pieces")))
            .child(self.render_board_section(cx))
    }
}
