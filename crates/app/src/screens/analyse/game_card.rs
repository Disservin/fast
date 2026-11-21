//! The game card at the top of the scoresheet: players, ratings, event, date, time
//! control and opening, with a form for editing the game's headers.

use super::*;

/// The headers the form edits, with their labels, in the order shown.
const FORM_TAGS: [(&str, &str); 10] = [
    ("White", "White"),
    ("WhiteElo", "White Elo"),
    ("Black", "Black"),
    ("BlackElo", "Black Elo"),
    ("Event", "Event"),
    ("Site", "Site"),
    ("Date", "Date"),
    ("Round", "Round"),
    ("TimeControl", "Time control"),
    ("Result", "Result"),
];

/// Headers that every PGN carries; an empty one is written as the PGN's unknown value.
const ROSTER: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

/// The open header form: one field per header, each writing to the game as it changes.
pub(super) struct HeaderForm {
    fields: Vec<(&'static str, &'static str, Entity<InputState>)>,
    _subscriptions: Vec<Subscription>,
}

/// A header's value, or `None` when it is missing or holds PGN's unknown marker.
pub(super) fn known_header<'a>(tree: &'a GameTree, tag: &str) -> Option<&'a str> {
    tree.header(tag)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().all(|c| matches!(c, '?' | '.')))
}

/// What a PGN result means for the badge: the score, or "Unfinished".
pub(super) fn result_label(result: Option<&str>) -> &'static str {
    match result {
        Some("1-0") => "1-0",
        Some("0-1") => "0-1",
        Some("1/2-1/2") | Some("½-½") => "½-½",
        _ => "Unfinished",
    }
}

/// The value PGN uses for an empty header: `?` for the required ones, nothing otherwise.
fn empty_value(tag: &str) -> &'static str {
    match tag {
        "Date" => "????.??.??",
        "Result" => "*",
        tag if ROSTER.contains(&tag) => "?",
        _ => "",
    }
}

impl AnalyseScreen {
    /// Opens the header form, or closes it when it is already open.
    pub(super) fn toggle_header_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.header_form.take().is_some() {
            cx.notify();
            return;
        }
        let mut fields = Vec::new();
        let mut subscriptions = Vec::new();
        for (tag, label) in FORM_TAGS {
            let value = known_header(&self.tree, tag).unwrap_or("").to_owned();
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder("—");
                state.set_value(value, window, cx);
                state
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().trim().to_owned();
                        let value = if value.is_empty() {
                            empty_value(tag).to_owned()
                        } else {
                            value
                        };
                        this.tree.set_header(tag, &value);
                        cx.notify();
                    }
                },
            ));
            fields.push((tag, label, input));
        }
        self.header_form = Some(HeaderForm {
            fields,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// The opening the game's main line reaches: its own Opening header, else the
    /// deepest named position along the main line.
    fn game_opening(&self) -> Option<(String, String)> {
        let classified = classify(
            self.tree
                .mainline()
                .iter()
                .map(|&id| self.tree.node(id).san.as_str()),
        );
        match known_header(&self.tree, "Opening") {
            Some(name) => Some((
                known_header(&self.tree, "ECO")
                    .map(str::to_owned)
                    .or_else(|| classified.as_ref().map(|o| o.eco.to_owned()))
                    .unwrap_or_default(),
                name.to_owned(),
            )),
            None => classified.map(|o| (o.eco.to_owned(), o.name.to_owned())),
        }
    }

    /// One player: a swatch in their piece colour, the name and the rating.
    fn render_card_player(&self, side: chess_core::Color, cx: &gpui::App) -> gpui::Div {
        let tag = if side.is_white() { "White" } else { "Black" };
        let theme = self.model.read(cx).config.board.prefs().theme;
        let name = known_header(&self.tree, tag);
        let elo = known_header(&self.tree, &format!("{tag}Elo"));
        h_flex()
            .h(px(24.))
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .size(px(10.))
                    .rounded(px(2.))
                    .bg(rgb(ui::board::piece_colour(theme, side)))
                    .border_as(T.border_strong),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .map(|el| match name {
                        Some(name) => el
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(T.fg))
                            .child(name.to_owned()),
                        None => el
                            .text_color(rgb(T.muted))
                            .child(ui::i18n::t("Name not set")),
                    }),
            )
            .child(
                mono(elo.unwrap_or("—").to_owned())
                    .flex_none()
                    .whitespace_nowrap()
                    .text_as(12., if elo.is_some() { T.fg_2 } else { T.faint }),
            )
    }

    /// The game card: a header with the result and the edit button, both players,
    /// event, date and time control, and the opening.
    pub(super) fn render_game_card(
        &self,
        actions: gpui::Div,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let result = result_label(self.tree.header("Result"));
        let figure = |caption: &'static str, tag: &str| {
            let value = known_header(&self.tree, tag);
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    div()
                        .text_as(10., T.faint)
                        .child(ui::i18n::t(caption).to_uppercase()),
                )
                .child(
                    mono(value.unwrap_or("—").to_owned())
                        .truncate()
                        .text_as(12., if value.is_some() { T.fg_2 } else { T.faint }),
                )
        };
        let opening = self.game_opening();
        v_flex()
            .flex_none()
            .border_b_as(T.border)
            .child(
                h_flex()
                    .h(px(44.))
                    .px(px(16.))
                    .gap(px(6.))
                    .border_b_as(T.divider)
                    .child(section_label("Game"))
                    .child(
                        ui::widgets::badge(ui::i18n::t(result))
                            .when(result != "Unfinished", |badge| badge.font_family(MONO)),
                    )
                    .when(self.tree.variant == chess_core::Variant::Chess960, |row| {
                        row.child(ui::widgets::badge("Chess960"))
                    })
                    .child(div().flex_1())
                    .child(
                        icon_btn("edit-headers", "icons/pencil.svg")
                            .mini()
                            .w(24.)
                            .pad(0.)
                            .selected(self.sheet_tab == SheetTab::Headers)
                            .tooltip(ui::i18n::t("Edit players, event and other headers"))
                            .on_click(this.listen(move |screen, window, cx| {
                                let tab = if screen.sheet_tab == SheetTab::Headers {
                                    SheetTab::Moves
                                } else {
                                    SheetTab::Headers
                                };
                                screen.set_sheet_tab(tab, window, cx)
                            })),
                    )
                    .child(actions),
            )
            .child(
                v_flex()
                    .px(px(16.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .gap(px(2.))
                    .child(self.render_card_player(chess_core::Color::White, cx))
                    .child(self.render_card_player(chess_core::Color::Black, cx))
                    .child(
                        h_flex()
                            .mt(px(6.))
                            .gap(px(10.))
                            .child(figure("Event", "Event"))
                            .child(figure("Date", "Date"))
                            .child(figure("Time control", "TimeControl")),
                    )
                    .child(
                        h_flex()
                            .mt(px(10.))
                            .px(px(10.))
                            .py(px(6.))
                            .gap(px(8.))
                            .rounded(px(4.))
                            .bg(rgb(T.surface_2))
                            .map(|row| match opening {
                                Some((eco, name)) => row
                                    .when(!eco.is_empty(), |row| {
                                        row.child(
                                            mono(eco)
                                                .flex_none()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_as(13., T.accent),
                                        )
                                    })
                                    .child(div().min_w_0().text_as(13., T.fg_2).child(name)),
                                None => row.child(
                                    div()
                                        .text_as(13., T.muted)
                                        .child(ui::i18n::t("Starting position")),
                                ),
                            }),
                    ),
            )
    }

    /// The header form, shown on the Headers tab.
    pub(super) fn render_header_form(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let form = self.header_form.as_ref()?;
        let this = cx.entity();
        let mut list = v_flex().px(px(16.)).py(px(10.)).gap(px(6.));
        for (_, label, input) in &form.fields {
            list = list.child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .col_w(88.)
                            .whitespace_nowrap()
                            .text_as(13., T.muted)
                            .child(ui::i18n::t(label)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(crate::controls::text_input(input).height(28.)),
                    ),
            );
        }
        Some(
            v_flex()
                .child(list)
                .child(h_flex().px(px(16.)).pb(px(12.)).justify_end().child(
                    btn("headers-done", "Done").compact().on_click(this.listen(
                        move |screen, window, cx| screen.set_sheet_tab(SheetTab::Moves, window, cx),
                    )),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_headers_count_as_missing() {
        let mut tree = GameTree::new();
        tree.set_header("Event", "?");
        tree.set_header("Date", "????.??.??");
        tree.set_header("White", "Carlsen");
        assert_eq!(known_header(&tree, "Event"), None);
        assert_eq!(known_header(&tree, "Date"), None);
        assert_eq!(known_header(&tree, "Black"), None);
        assert_eq!(known_header(&tree, "White"), Some("Carlsen"));
    }

    #[test]
    fn results_read_as_a_score_or_unfinished() {
        assert_eq!(result_label(Some("1-0")), "1-0");
        assert_eq!(result_label(Some("1/2-1/2")), "½-½");
        assert_eq!(result_label(Some("*")), "Unfinished");
        assert_eq!(result_label(None), "Unfinished");
        assert_eq!(empty_value("Date"), "????.??.??");
        assert_eq!(empty_value("WhiteElo"), "");
    }
}
