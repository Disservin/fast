//! The position note: free text kept with the position in every game, and quick tags.

use super::*;

/// Tags a note can be given with one click.
const TAGS: [&str; 4] = ["Kingside attack", "IQP", "Critical", "Review later"];

/// The note with `tag` added as `[tag]` on its own last line, or taken out when it is
/// already there.
pub(super) fn toggle_tag(note: &str, tag: &str) -> String {
    let token = format!("[{tag}]");
    if note.contains(&token) {
        let removed = note.replace(&token, "");
        // Tidy the spaces and blank lines the tag leaves behind.
        let lines: Vec<String> = removed
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        return lines.join("\n").trim_end().to_owned();
    }
    let trimmed = note.trim_end();
    match trimmed.lines().last() {
        None => token,
        // Tags gather on one line after the text.
        Some(last) if last.trim_start().starts_with('[') => format!("{trimmed} {token}"),
        Some(_) => format!("{trimmed}\n{token}"),
    }
}

impl AnalyseScreen {
    /// Adds a quick tag to the note, or takes it out, and saves the note.
    fn toggle_note_tag(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = toggle_tag(&self.note.read(cx).value(), tag);
        self.note
            .update(cx, |note, cx| note.set_value(text.clone(), window, cx));
        self.model.read(cx).set_note(self.note_hash, &text);
        cx.notify();
    }

    /// The Position note panel: the note, where it is kept, and the quick tags.
    pub(super) fn render_position_note(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let text = self.note.read(cx).value().to_string();
        let mut tags = h_flex().flex_wrap().gap(px(4.)).mt(px(8.));
        for (index, tag) in TAGS.into_iter().enumerate() {
            let on = text.contains(&format!("[{tag}]"));
            let label = if on {
                ui::i18n::t(tag).to_owned()
            } else {
                format!("+ {}", ui::i18n::t(tag))
            };
            tags =
                tags.child(
                    btn(("note-tag", index), label)
                        .mini()
                        .selected(on)
                        .tooltip(if on {
                            "Remove this tag from the note"
                        } else {
                            "Add this tag to the note"
                        })
                        .on_click(this.listen(move |screen, window, cx| {
                            screen.toggle_note_tag(tag, window, cx)
                        })),
                );
        }
        panel()
            .rounded_none()
            .flex_none()
            .p(px(16.))
            .child(
                h_flex()
                    .mb(px(8.))
                    .gap(px(8.))
                    .child(section_label("Position note"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_as(11., T.muted)
                            .child(ui::i18n::t("Saved to this position in all games")),
                    ),
            )
            .child(
                div().bg(rgb(T.bg)).rounded(px(4.)).child(
                    gpui_component::input::Textarea::new(&self.note)
                        .h(px(64.))
                        .text_size(px(13.)),
                ),
            )
            .child(tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_go_on_their_own_line_and_come_off_again() {
        let note = toggle_tag("Push h-pawn", "Critical");
        assert_eq!(note, "Push h-pawn\n[Critical]");
        let note = toggle_tag(&note, "IQP");
        assert_eq!(note, "Push h-pawn\n[Critical] [IQP]");
        let note = toggle_tag(&note, "Critical");
        assert_eq!(note, "Push h-pawn\n[IQP]");
        assert_eq!(toggle_tag("", "IQP"), "[IQP]");
        assert_eq!(toggle_tag("[IQP]", "IQP"), "");
    }
}
