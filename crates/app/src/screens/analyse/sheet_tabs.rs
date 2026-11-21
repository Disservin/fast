//! The tabs over the scoresheet (Moves, Notes and Headers), the Notes list and the
//! annotation toolbar under the moves.

use super::*;

/// What the scoresheet column shows under the game card.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SheetTab {
    /// The moves with their comments and variations.
    #[default]
    Moves,
    /// Every comment in the game, each with the move it belongs to.
    Notes,
    /// The form for the game's players, event and other headers.
    Headers,
}

/// The annotation symbols offered under the moves, good to bad.
const NAG_BUTTONS: [(&str, u8, &str); 6] = [
    ("!", 1, "Good move"),
    ("!!", 3, "Brilliant move"),
    ("!?", 5, "Interesting move"),
    ("?!", 6, "Dubious move"),
    ("?", 2, "Mistake"),
    ("??", 4, "Blunder"),
];

impl AnalyseScreen {
    /// Shows a tab of the scoresheet column, opening the header form for Headers.
    pub(super) fn set_sheet_tab(
        &mut self,
        tab: SheetTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet_tab = tab;
        if (tab == SheetTab::Headers) != self.header_form.is_some() {
            self.toggle_header_form(window, cx);
        }
        cx.notify();
    }

    /// The tab row: Moves, Notes and Headers, with the Table or Text layout of the
    /// moves on the right.
    pub(super) fn render_sheet_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut tabs = h_flex().h_full().gap(px(14.));
        for (index, (tab, label)) in [
            (SheetTab::Moves, "Moves"),
            (SheetTab::Notes, "Notes"),
            (SheetTab::Headers, "Headers"),
        ]
        .into_iter()
        .enumerate()
        {
            tabs = tabs.child(header_tab(
                ("sheet-tab", index),
                label,
                self.sheet_tab == tab,
                this.listen(move |screen, window, cx| screen.set_sheet_tab(tab, window, cx)),
            ));
        }
        let style = self.model.read(cx).config.view.scoresheet;
        let layout = |id: &'static str, label: &'static str, value: ScoresheetStyle, tip| {
            btn(id, label)
                .mini()
                .pad(7.)
                .selected(style == value)
                .tooltip(tip)
                .on_click(this.listen(move |screen, _, cx| {
                    screen.set_view(cx, |view| view.scoresheet = value);
                }))
        };
        h_flex()
            .flex_none()
            .h(px(40.))
            .px(px(16.))
            .gap(px(4.))
            .border_b_as(T.border)
            .child(tabs)
            .child(div().flex_1())
            .when(self.sheet_tab == SheetTab::Moves, |row| {
                row.child(layout(
                    "sheet-table",
                    "Table",
                    ScoresheetStyle::Table,
                    "One row per move, as on a scoresheet",
                ))
                .child(layout(
                    "sheet-text",
                    "Text",
                    ScoresheetStyle::Inline,
                    "Running text with variations in brackets",
                ))
            })
    }

    /// Every comment in the game in move order, side lines after the move they leave
    /// from. Clicking one goes to its move.
    pub(super) fn render_notes_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let notation = self.model.read(cx).config.board.notation;
        let mut list = v_flex();
        let mut stack = vec![GameTree::ROOT];
        let mut count = 0;
        while let Some(id) = stack.pop() {
            let node = self.tree.node(id);
            stack.extend(node.children.iter().rev().copied());
            let text = chess_core::strip_commands(&node.comment);
            if text.is_empty() {
                continue;
            }
            count += 1;
            let label = if id == GameTree::ROOT {
                ui::i18n::t("Start").to_owned()
            } else {
                let san = chess_core::display_san(&node.san, node.mv.as_ref(), notation);
                chess_core::notation::numbered(node.ply, &san)
            };
            list = list.child(
                v_flex()
                    .id(("note-row", id))
                    .px(px(16.))
                    .py(px(8.))
                    .gap(px(2.))
                    .border_b_as(T.divider)
                    .cursor_pointer()
                    .when(id == self.current, |row| row.bg(rgb(T.selected)))
                    .hover(|row| row.bg(rgb(T.hover)))
                    .on_click(
                        this.listen_event(move |screen, _, window, cx| screen.jump(id, window, cx)),
                    )
                    .child(mono(label).text_as(12., T.muted))
                    .child(div().text_as(13., T.fg_2).child(text.to_owned())),
            );
        }
        if count == 0 {
            return ui::widgets::note("No comments yet. Select a move and press Comment.")
                .into_any_element();
        }
        list.into_any_element()
    }

    /// The annotation toolbar: the move symbols in their colours, then Comment, Promote
    /// variation and Delete from here.
    pub(super) fn render_annotation_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let at_root = self.current == GameTree::ROOT;
        let nags = self.tree.node(self.current).nags.clone();
        let mut symbols = h_flex().gap(px(2.));
        for (index, (label, nag, tip)) in NAG_BUTTONS.into_iter().enumerate() {
            symbols = symbols.child(
                btn(("nag", index), label)
                    .compact()
                    .mono()
                    .w(26.)
                    .pad(0.)
                    // A chosen symbol sits on `selected`, where only `fg` is readable in
                    // every palette.
                    .when(!nags.contains(&nag), |button| {
                        button.color(ui::scoresheet::nag_color(nag))
                    })
                    .selected(nags.contains(&nag))
                    .disabled(at_root)
                    .tooltip(tip)
                    .on_click(this.listen(move |screen, _, cx| screen.annotate(nag, cx))),
            );
        }
        let on_mainline = self.tree.mainline().contains(&self.current);
        h_flex()
            .flex_none()
            .flex_wrap()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .border_t_as(T.border)
            .child(symbols)
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(2.))
                    .child(
                        icon_btn("comment", "icons/message-square.svg")
                            .compact()
                            .w(28.)
                            .pad(0.)
                            .selected(self.editing_comment.is_some())
                            .tooltip("Comment on this move")
                            .on_click(this.listen(move |screen, window, cx| {
                                screen.toggle_comment_editor(window, cx)
                            })),
                    )
                    .child(
                        icon_btn("promote", "icons/arrow-up.svg")
                            .compact()
                            .w(28.)
                            .pad(0.)
                            .disabled(on_mainline)
                            .tooltip("Promote variation")
                            .on_click(this.listen(move |screen, _, cx| {
                                let current = screen.current;
                                screen.promote(current, cx);
                            })),
                    )
                    .child(
                        icon_btn("delete-from-here", "icons/trash.svg")
                            .compact()
                            .w(28.)
                            .pad(0.)
                            .disabled(at_root)
                            .tooltip("Delete from here")
                            .on_click(this.listen(move |screen, window, cx| {
                                let current = screen.current;
                                screen.delete_from(current, window, cx);
                            })),
                    ),
            )
    }

    /// Opens the comment field for the current move, or closes it.
    fn toggle_comment_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_comment = if self.editing_comment.is_some() {
            None
        } else {
            Some(self.current)
        };
        let text = chess_core::strip_commands(&self.tree.node(self.current).comment);
        self.comment.update(cx, |state, cx| {
            state.set_value(text, window, cx);
        });
        cx.notify();
    }
}
