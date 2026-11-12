//! A separate window with an engine's raw UCI traffic, for engines in Debug mode.

use std::{ops::Range, time::Duration};

use engine::{EngineLog, LogKind, LogLine};
use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, IntoElement,
    ListHorizontalSizingBehavior, Render, SharedString, Task, TitlebarOptions,
    UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    rgb, size, uniform_list,
};
use gpui_component::{Root, scroll::ScrollableElement};
use ui::{
    MONO, SANS, T,
    widgets::{TextExt, btn, h_flex, v_flex},
};

/// How often the window looks for new output.
const POLL: Duration = Duration::from_millis(200);

pub struct DebugWindow {
    title: SharedString,
    log: EngineLog,
    lines: Vec<LogLine>,
    revision: Option<u64>,
    follow: bool,
    show_info: bool,
    scroll: UniformListScrollHandle,
    _poll: Task<()>,
}

impl DebugWindow {
    /// Opens a window following `log`.
    pub fn open(name: &str, log: EngineLog, cx: &mut App) {
        let title: SharedString = format!("{name} engine output").into();
        let bounds = Bounds::centered(None, size(px(760.), px(540.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.clone()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| Self::new(title, log, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            log::error!("could not open the debug window: {err:#}");
        }
    }

    fn new(title: SharedString, log: EngineLog, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if Some(this.log.revision()) != this.revision {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        Self {
            title,
            log,
            lines: Vec::new(),
            revision: None,
            follow: true,
            show_info: true,
            scroll: UniformListScrollHandle::new(),
            _poll: poll,
        }
    }

    fn refresh(&mut self) {
        let revision = self.log.revision();
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        let lines = self.log.snapshot(self.show_info);
        if !self.follow {
            keep_log_anchor(&self.scroll, &self.lines, &lines);
        }
        self.lines = lines;
        if self.follow && !self.lines.is_empty() {
            self.scroll
                .scroll_to_item(self.lines.len() - 1, gpui::ScrollStrategy::Bottom);
        }
    }

    fn row(&self, index: usize) -> gpui::AnyElement {
        let Some(line) = self.lines.get(index) else {
            return div().into_any_element();
        };
        log_row(line.kind)
            .child(SharedString::from(line.text.clone()))
            .into_any_element()
    }
}

/// Height of one engine log line.
pub(crate) const LOG_ROW_H: f32 = 19.;

/// Keeps the line at the top of a log view in place when the log is replaced by a
/// newer snapshot, so reading back through it is not disturbed while old lines fall
/// off the front and new ones arrive at the end.
pub(crate) fn keep_log_anchor(scroll: &UniformListScrollHandle, old: &[LogLine], new: &[LogLine]) {
    let state = scroll.0.borrow();
    let row = state
        .last_item_size
        .map_or(LOG_ROW_H, |size| f32::from(size.item.height))
        .max(1.);
    let mut offset = state.base_handle.offset();
    let top = (-f32::from(offset.y) / row).floor().max(0.) as usize;
    let Some(anchor) = old.get(top).map(|line| line.seq) else {
        return;
    };
    let moved = new.partition_point(|line| line.seq < anchor) as f32 - top as f32;
    if moved != 0. {
        offset.y -= px(moved * row);
        state.base_handle.set_offset(offset);
    }
}

/// One engine log line, coloured by direction and starting with its mark; the caller
/// adds the text.
pub(crate) fn log_row(kind: LogKind) -> gpui::Div {
    let (mark, color) = match kind {
        LogKind::Sent => (">", T.fg),
        LogKind::Received => ("<", T.fg_2),
        LogKind::Stderr => ("!", T.warn),
        LogKind::System => ("*", T.muted),
    };
    h_flex()
        .min_w_full()
        .h(px(LOG_ROW_H))
        .px(px(12.))
        .gap(px(8.))
        .whitespace_nowrap()
        .font_family(MONO)
        .text_size(px(12.))
        .text_color(rgb(color))
        .child(div().flex_none().text_color(rgb(T.faint)).child(mark))
}

impl Render for DebugWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh();
        let count = self.lines.len();
        let longest = self
            .lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.text.len())
            .map(|(index, _)| index);
        let this = cx.entity();
        let (follow_this, info_this, clear_this, copy_this) =
            (this.clone(), this.clone(), this.clone(), this);
        v_flex()
            .size_full()
            .bg(rgb(T.bg))
            .text_color(rgb(T.fg))
            .font_family(SANS)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(48.))
                    .px(px(14.))
                    .gap(px(6.))
                    .justify_between()
                    .bg(rgb(T.surface))
                    .border_b_1()
                    .border_color(rgb(T.border))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.title.clone()),
                            )
                            .child(div().text_as(12., T.muted).child(format!("{count} lines"))),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(
                                btn("debug-info", "Info lines")
                                    .compact()
                                    .selected(self.show_info)
                                    .on_click(move |_, cx| {
                                        info_this.update(cx, |this, cx| {
                                            this.show_info = !this.show_info;
                                            this.revision = None;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                btn("debug-follow", "Follow")
                                    .compact()
                                    .selected(self.follow)
                                    .on_click(move |_, cx| {
                                        follow_this.update(cx, |this, cx| {
                                            this.follow = !this.follow;
                                            this.revision = None;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(btn("debug-copy", "Copy all").compact().on_click(
                                move |_, cx| {
                                    let text = copy_this
                                        .read(cx)
                                        .lines
                                        .iter()
                                        .map(|line| line.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                },
                            ))
                            .child(
                                btn("debug-clear", "Clear")
                                    .compact()
                                    .on_click(move |_, cx| {
                                        clear_this.update(cx, |this, cx| {
                                            this.log.clear();
                                            cx.notify();
                                        });
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .py(px(6.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.follow = false;
                            cx.notify();
                        }),
                    )
                    .child(
                        uniform_list(
                            "debug-log",
                            count,
                            cx.processor(|this, range: Range<usize>, _, _| {
                                range.map(|index| this.row(index)).collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .with_horizontal_sizing_behavior(
                            ListHorizontalSizingBehavior::Unconstrained,
                        )
                        .with_width_from_item(longest)
                        .on_scroll(cx.listener(|this, _, _, cx| {
                            this.follow = false;
                            cx.notify();
                        }))
                        .size_full(),
                    )
                    .vertical_scrollbar(&self.scroll)
                    .horizontal_scrollbar(&self.scroll),
            )
    }
}
