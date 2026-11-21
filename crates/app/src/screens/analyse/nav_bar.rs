//! The bar under the board: move navigation with auto-play, a scrubber along the line,
//! and the board actions (flip, edit position, copy and image).

use super::*;

/// How long auto-play shows each position.
const AUTOPLAY_STEP: std::time::Duration = std::time::Duration::from_millis(1000);

impl AnalyseScreen {
    /// A tooltip with the shortcut for the action, when one is set: "Flip board (F)".
    fn shortcut_tip(&self, label: &str, action: &str, cx: &gpui::App) -> String {
        let label = ui::i18n::t(label);
        match self.model.read(cx).config.shortcuts.get(action) {
            Some(key) if !key.is_empty() => format!("{label} ({})", key_name(key)),
            _ => label.to_owned(),
        }
    }

    /// Starts stepping through the line one move a second, or stops it.
    pub(super) fn toggle_autoplay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autoplay.take().is_some() {
            cx.notify();
            return;
        }
        // Starting at the end of the line replays it from the beginning.
        if self.tree.node(self.current).children.is_empty() {
            self.go_first(window, cx);
        }
        self.autoplay = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(AUTOPLAY_STEP).await;
                let going = this.update_in(cx, |screen, window, cx| {
                    if screen.tree.node(screen.current).children.is_empty() {
                        screen.autoplay = None;
                        cx.notify();
                        return false;
                    }
                    screen.step(1, window, cx);
                    true
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        }));
        cx.notify();
    }

    /// Moves the scrubber to the shown position in the line.
    pub(super) fn sync_scrubber(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.timeline_ids();
        let index = line
            .iter()
            .position(|id| *id == self.current)
            .map_or(0, |i| i + 1);
        let fraction = index as f32 / line.len().max(1) as f32;
        if (self.scrubber.read(cx).value().start() - fraction).abs() > 1e-4 {
            self.scrubber
                .update(cx, |state, cx| state.set_value(fraction, window, cx));
        }
    }

    /// Goes to the position the scrubber was dragged to.
    pub(super) fn scrub_to(&mut self, fraction: f32, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.timeline_ids();
        let index = (fraction * line.len() as f32).round() as usize;
        let target = match index {
            0 => GameTree::ROOT,
            index => line[(index - 1).min(line.len().saturating_sub(1))],
        };
        if target != self.current && (index == 0 || !line.is_empty()) {
            self.jump(target, window, cx);
        }
    }

    /// The navigation bar: first, previous, auto-play, next and last on the left, the
    /// scrubber with the move counter in the middle, and the board actions on the right.
    pub(super) fn render_nav_bar(
        &self,
        board_size: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let width = 32.;
        let nav = |id: &'static str,
                   icon: &'static str,
                   tip: String,
                   action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            icon_btn(id, icon)
                .w(width)
                .h(32.)
                .tooltip(tip)
                .on_click(this.listen(action))
        };
        let playing = self.autoplay.is_some();
        let ply = self.tree.node(self.current).ply;
        let last = self
            .timeline_ids()
            .last()
            .map_or(ply, |id| self.tree.node(*id).ply)
            .max(ply);
        // Where the shown position sits in the line, in full moves.
        let counter = mono(format!(
            "{} {} / {}",
            ui::i18n::t("Move"),
            ply.div_ceil(2),
            last.div_ceil(2)
        ))
        .flex_none()
        .whitespace_nowrap()
        .text_as(12., T.muted);
        let flipped = self.board.read(cx).is_flipped();
        h_flex()
            .mt(px(6.))
            .gap(px(4.))
            .child(nav(
                "first",
                "icons/skip-back.svg",
                self.shortcut_tip("First move", "first_move", cx),
                Self::go_first,
            ))
            .child(nav(
                "prev",
                "icons/chevron-left.svg",
                self.shortcut_tip("Previous move", "prev_move", cx),
                |s, w, c| s.step(-1, w, c),
            ))
            .child(
                icon_btn(
                    "autoplay",
                    if playing {
                        "icons/pause.svg"
                    } else {
                        "icons/play.svg"
                    },
                )
                .w(width)
                .h(32.)
                .selected(playing)
                .tooltip(if playing {
                    "Stop playing through the moves"
                } else {
                    "Play through the moves"
                })
                .on_click(this.listen(Self::toggle_autoplay)),
            )
            .child(nav(
                "next",
                "icons/chevron-right.svg",
                self.shortcut_tip("Next move", "next_move", cx),
                |s, w, c| s.step(1, w, c),
            ))
            .child(nav(
                "last",
                "icons/skip-forward.svg",
                self.shortcut_tip("Last move", "last_move", cx),
                Self::go_last,
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .px(px(8.))
                    .gap(px(10.))
                    .when(board_size >= px(480.), |row| {
                        row.child(
                            div().flex_1().min_w_0().child(
                                gpui_component::slider::Slider::new(&self.scrubber)
                                    .accessibility_label(ui::i18n::t("Position in the line")),
                            ),
                        )
                    })
                    .when(board_size >= px(560.), |row| row.child(counter)),
            )
            .child(
                icon_btn("flip", "icons/arrow-down-up.svg")
                    .w(width)
                    .h(32.)
                    .selected(flipped)
                    .tooltip(self.shortcut_tip("Flip board", "flip_board", cx))
                    .on_click(this.listen(move |screen, _, cx| screen.flip(cx))),
            )
            .child(
                icon_btn("edit-position", "icons/pencil.svg")
                    .w(width)
                    .h(32.)
                    .selected(self.editor.is_some())
                    .tooltip(ui::i18n::t("Edit position"))
                    .on_click(this.listen(move |screen, _, cx| screen.toggle_editor(cx))),
            )
            .child(self.render_copy_menu(width, cx))
            .child(
                icon_btn("snapshot", "icons/camera.svg")
                    .w(width)
                    .h(32.)
                    .tooltip(ui::i18n::t("Copy the board as an image and save it"))
                    .on_click(this.listen(move |screen, _, cx| screen.snapshot_board(cx))),
            )
    }
}

/// A shortcut as shown in a tooltip: "Home", "←", "F".
fn key_name(key: &str) -> String {
    match key {
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        key => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_names_read_like_key_caps() {
        assert_eq!(key_name("home"), "Home");
        assert_eq!(key_name("left"), "←");
        assert_eq!(key_name("f"), "F");
    }
}
