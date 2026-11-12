//! The Log panel: each engine's UCI traffic, with selection and copying by mouse and keyboard.

use super::*;

/// Text selected in the Log panel, as (line, byte) positions in `log_lines`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogSelection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl LogSelection {
    fn at(position: (usize, usize)) -> Self {
        Self {
            anchor: position,
            head: position,
        }
    }

    fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    /// The selected bytes of one line, if any of it is selected.
    fn range_in(&self, line: usize, text: &str) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if line < start.0 || line > end.0 {
            return None;
        }
        let from = if line == start.0 { start.1 } else { 0 };
        let to = if line == end.0 { end.1 } else { text.len() };
        let range = floor_char(text, from)..floor_char(text, to);
        (range.start < range.end).then_some(range)
    }

    /// The selected text, one log line per text line.
    pub fn text(&self, lines: &[LogLine]) -> String {
        let (start, end) = self.ordered();
        (start.0..=end.0.min(lines.len().saturating_sub(1)))
            .filter_map(|index| {
                let text = &lines.get(index)?.text;
                let range = self.range_in(index, text).unwrap_or(0..0);
                Some(&text[range])
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The nearest char boundary at or before `index`, clamped to the text.
pub(super) fn floor_char(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

impl AnalyseScreen {
    /// The log of the engine the panels show, if it is in analysis.
    pub(super) fn panel_log(&self, cx: &gpui::App) -> Option<(EngineId, engine::EngineLog)> {
        let chosen = self.panel_engine_config(cx)?.id;
        self.live
            .read(cx)
            .engines
            .iter()
            .find(|engine| engine.config.id == chosen)
            .map(|engine| (chosen, engine.log.clone()))
    }

    /// Takes a new snapshot only when the log or the filter changed. A selection holds
    /// the log still, so the text under it cannot move, until it is cleared.
    pub(super) fn refresh_log(&mut self, cx: &gpui::App) {
        let key = self.current_log_key(cx);
        let same_engine = key.map(|key| key.0) == self.log_key.map(|key| key.0);
        if self.log_selection.is_some() && same_engine {
            return;
        }
        self.log_selection = None;
        if key.is_some() && key == self.log_key {
            return;
        }
        let same_view = same_engine && key.map(|key| key.2) == self.log_key.map(|key| key.2);
        self.log_key = key;
        let lines = self
            .panel_log(cx)
            .map(|(_, log)| log.snapshot(self.log_show_info))
            .unwrap_or_default();
        // Reading back through the log: new output must not move the text in view.
        if !self.log_follow && same_view {
            crate::debug_window::keep_log_anchor(&self.log_scroll, &self.log_lines, &lines);
        }
        self.log_lines = lines;
    }

    /// The byte under the pointer in a visible log line.
    pub(super) fn log_index_at(&self, line: usize, position: Point<Pixels>) -> usize {
        self.log_layouts.borrow().get(&line).map_or(0, |layout| {
            match layout.index_for_position(position) {
                Ok(index) | Err(index) => index,
            }
        })
    }

    pub(super) fn render_log_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(line) = self.log_lines.get(index) else {
            return div().into_any_element();
        };
        let selected = self
            .log_selection
            .and_then(|selection| selection.range_in(index, &line.text));
        let text = StyledText::new(SharedString::from(line.text.clone())).with_highlights(
            selected.map(|range| {
                (
                    range,
                    HighlightStyle {
                        background_color: Some(rgb(T.selected).into()),
                        ..Default::default()
                    },
                )
            }),
        );
        self.log_layouts
            .borrow_mut()
            .insert(index, text.layout().clone());
        // Rows take their text's width; the list scrolls sideways to the longest.
        crate::debug_window::log_row(line.kind)
            .id(("log-line", index))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    let at = (index, this.log_index_at(index, event.position));
                    this.log_selection = Some(LogSelection::at(at));
                    this.log_selecting = true;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if !this.log_selecting || !event.dragging() {
                    return;
                }
                let head = (index, this.log_index_at(index, event.position));
                if let Some(selection) = &mut this.log_selection
                    && selection.head != head
                {
                    selection.head = head;
                    cx.notify();
                }
            }))
            .child(text)
            .into_any_element()
    }

    /// Ends a drag. A click without a drag clears the selection and lets the log move.
    pub(super) fn end_log_selection(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.log_selecting) {
            return;
        }
        if self
            .log_selection
            .is_some_and(|selection| selection.is_empty())
        {
            self.log_selection = None;
        }
        cx.notify();
    }

    /// Esc clears the log selection and Ctrl+C copies it. Returns whether the key was used.
    pub(super) fn handle_log_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.log_selection.filter(|_| self.log_panel_open) else {
            return false;
        };
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.log_selection = None;
            cx.notify();
            true
        } else if keystroke.key == "c" && keystroke.modifiers.secondary(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(selection.text(&self.log_lines)));
            self.model.toast("Selection copied.", cx);
            true
        } else {
            false
        }
    }
}

impl AnalyseScreen {
    pub(crate) fn render_log_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_log(cx);
        let this = cx.entity();
        let count = self.log_lines.len();
        let holding = self.log_selection.is_some();
        if self.log_follow && count > 0 && !holding {
            self.log_scroll
                .scroll_to_item(count - 1, ScrollStrategy::Bottom);
        }
        let name = self
            .panel_engine_config(cx)
            .map_or_else(|| "No engine".to_owned(), |config| config.name);
        let tools = h_flex()
            .h(px(40.))
            .px(px(16.))
            .gap(px(10.))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(section_label("Log"))
            .child(div().text_as(12., T.muted).child(name))
            .when(holding, |row| {
                row.child(div().text_as(12., T.warn).child(ui::i18n::t(
                    "Held while selected. Ctrl+C copies, Esc releases.",
                )))
            })
            .child(div().flex_1())
            .child(
                div().flex_none().w(px(64.)).child(
                    check("log-info", self.log_show_info, "Info")
                        .text(13.)
                        .on_change(this.listen_with(move |screen, value, _, cx| {
                            screen.log_selection = None;
                            screen.log_show_info = value;
                            cx.notify();
                        })),
                ),
            )
            .child(
                div().flex_none().w(px(80.)).child(
                    check("log-follow", self.log_follow, "Follow")
                        .text(13.)
                        .on_change(this.listen_with(move |screen, value, _, cx| {
                            screen.log_follow = value;
                            cx.notify();
                        })),
                ),
            )
            .child(
                btn("log-copy", ui::i18n::t("Copy"))
                    .compact()
                    .on_click(this.listen(move |screen, _, cx| {
                        // The selection if there is one, else the whole log.
                        let (text, message) = match screen.log_selection {
                            Some(selection) => {
                                (selection.text(&screen.log_lines), "Selection copied.")
                            }
                            None => (
                                screen
                                    .log_lines
                                    .iter()
                                    .map(|line| line.text.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                "Log copied.",
                            ),
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        screen.model.toast(message, cx);
                    })),
            )
            .child(
                btn("log-window", ui::i18n::t("Window"))
                    .compact()
                    .tooltip(ui::i18n::t(
                        "Follow this engine's raw output in its own window",
                    ))
                    .on_click({
                        let this = cx.entity();
                        move |_, cx| this.update(cx, |screen, cx| screen.open_engine_output(cx))
                    }),
            )
            .child(
                btn("log-clear", ui::i18n::t("Clear"))
                    .compact()
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.log_selection = None;
                        let chosen = screen.panel_engine_config(cx).map(|config| config.id);
                        if let Some(engine) = screen
                            .live
                            .read(cx)
                            .engines
                            .iter()
                            .find(|engine| Some(engine.config.id) == chosen)
                        {
                            engine.log.clear();
                        }
                        cx.notify();
                    })),
            );
        let longest = self
            .log_lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.text.len())
            .map(|(index, _)| index);
        let body: AnyElement = if count == 0 {
            div()
                .p(px(16.))
                .text_as(13., T.muted)
                .child(ui::i18n::t(
                    "Nothing logged yet. Start the engine to see its UCI traffic.",
                ))
                .into_any_element()
        } else {
            uniform_list(
                "engine-log",
                count,
                cx.processor(|this, range: Range<usize>, _, cx| {
                    // Only the rows just built can be hit-tested.
                    this.log_layouts.borrow_mut().clear();
                    range
                        .map(|index| this.render_log_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.log_scroll)
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .with_width_from_item(longest)
            .h(px(LOG_ROW_H * 12.))
            .on_scroll(cx.listener(|this, _, _, cx| {
                this.log_follow = false;
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_log_selection(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_log_selection(cx)),
            )
            .into_any_element()
        };
        panel()
            .rounded_none()
            .flex_none()
            .overflow_hidden()
            .child(tools)
            .child(
                div()
                    .relative()
                    .py(px(6.))
                    .bg(rgb(T.surface_2))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.log_follow = false;
                            cx.notify();
                        }),
                    )
                    .child(body)
                    .vertical_scrollbar(&self.log_scroll)
                    .horizontal_scrollbar(&self.log_scroll),
            )
            .into_any_element()
    }
}

impl AnalyseScreen {
    pub(crate) fn current_log_key(&self, cx: &gpui::App) -> Option<(EngineId, u64, bool)> {
        self.panel_log(cx)
            .map(|(id, log)| (id, log.revision(), self.log_show_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::LogKind;

    fn line(text: &str) -> LogLine {
        LogLine {
            kind: LogKind::Received,
            text: text.to_owned(),
            seq: 0,
        }
    }

    #[test]
    fn log_selection_spans_lines_in_either_direction() {
        let lines = [line("uciok"), line("readyok"), line("bestmove e2e4")];
        let forward = LogSelection {
            anchor: (0, 3),
            head: (2, 8),
        };
        assert_eq!(forward.text(&lines), "ok\nreadyok\nbestmove");
        let backward = LogSelection {
            anchor: forward.head,
            head: forward.anchor,
        };
        assert_eq!(backward.text(&lines), forward.text(&lines));
        let within = LogSelection {
            anchor: (2, 9),
            head: (2, 40),
        };
        assert_eq!(within.text(&lines), "e2e4");
        assert!(LogSelection::at((1, 2)).is_empty());
    }
}
