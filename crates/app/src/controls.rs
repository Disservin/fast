//! Thin wrappers styling the component library's stateful inputs for Fast.

use gpui::{
    AnyElement, App, Div, Entity, Focusable as _, IntoElement, RenderOnce, SharedString, Window,
    div, prelude::*, px, rgb,
};
use gpui_component::{
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
};
use ui::{T, widgets::h_flex};

pub type TextSelect = Entity<SelectState<SearchableVec<SharedString>>>;

/// A single-line input in a dark well: 36px tall, radius 4, fg border when focused.
#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    height: f32,
    suffix: Option<SharedString>,
    icon: Option<&'static str>,
    hint: Option<AnyElement>,
    invalid: bool,
    mono: bool,
}

pub fn text_input(state: &Entity<InputState>) -> TextField {
    TextField {
        state: state.clone(),
        height: 34.,
        suffix: None,
        icon: None,
        hint: None,
        invalid: false,
        mono: false,
    }
}

impl TextField {
    /// Uses aligned data typography for numbers and FENs.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn suffix(mut self, suffix: Option<impl Into<SharedString>>) -> Self {
        self.suffix = suffix.map(Into::into);
        self
    }

    /// Shows a small icon before the text, such as a magnifier on a search box.
    pub fn icon(mut self, path: &'static str) -> Self {
        self.icon = Some(path);
        self
    }

    /// Marks the field's value as unreadable with a `warn` outline.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Shows a hint at the right end, such as the shortcut that focuses the field. It hides
    /// while the field has focus.
    pub fn hint(mut self, hint: impl IntoElement) -> Self {
        self.hint = Some(hint.into_any_element());
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        div()
            .h(px(self.height))
            .w_full()
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(rgb(T.bg))
            .border_1()
            .when(focused, |field| field.border_2())
            .border_color(rgb(if self.invalid {
                T.warn
            } else if focused {
                T.accent
            } else {
                T.border_strong
            }))
            .child(
                Input::new(&self.state)
                    .when(self.mono, |input| input.font_family(ui::MONO))
                    .appearance(false)
                    .text_size(px(13.))
                    .when_some(self.icon, |input, path| {
                        input.prefix(ui::widgets::icon(path, px(14.), T.muted))
                    })
                    .when_some(self.suffix, |input, suffix| {
                        input.suffix(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(T.muted))
                                .child(suffix),
                        )
                    })
                    .when_some(self.hint.filter(|_| !focused), |input, hint| {
                        input.suffix(hint)
                    }),
            )
    }
}

/// The unit after a hash size field: MB, with the size in GB beside it once it
/// reaches 1024 MB, so 10000 reads as 9.8 GB.
pub fn hash_suffix(value: &str) -> SharedString {
    match value.trim().parse::<i64>() {
        Ok(mb) if mb >= 1024 => format!("MB ({})", engine::format_hash(mb)).into(),
        _ => "MB".into(),
    }
}

pub fn new_input(
    value: impl Into<SharedString>,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(ui::i18n::t(placeholder));
        state.set_value(value, window, cx);
        state
    })
}

pub fn new_select(
    items: Vec<SharedString>,
    selected: usize,
    window: &mut Window,
    cx: &mut App,
) -> TextSelect {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(
                items
                    .into_iter()
                    .map(|item| ui::i18n::t(&item).to_owned().into())
                    .collect::<Vec<SharedString>>(),
            ),
            Some(gpui_component::IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

pub fn select_el(state: &TextSelect) -> impl IntoElement + use<> {
    div()
        .w_full()
        .bg(rgb(T.bg))
        .rounded(px(4.))
        .text_size(px(14.))
        .child(Select::new(state).h(px(34.)).text_size(px(13.5)))
}

/// A small form label above a field.
pub fn field(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .min_w_0()
        .child(
            h_flex()
                .text_size(px(12.5))
                .text_color(rgb(T.muted))
                .child(ui::i18n::t(&label.into()).to_owned()),
        )
        .child(control)
}

/// An engine's picture: its custom icon if the file exists, otherwise a rook.
pub fn engine_icon(config: &engine::EngineConfig, size: f32) -> gpui::AnyElement {
    let frame = div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .overflow_hidden();
    match config.icon.as_ref().filter(|path| path.is_file()) {
        Some(path) => frame
            .child(gpui::img(path.clone()).size_full())
            .into_any_element(),
        None => frame
            .bg(rgb(T.bg))
            .border_1()
            .border_color(rgb(T.border_strong))
            .child(
                gpui::svg()
                    .path("pieces/merida/wR.svg")
                    .size(px(size * 0.72))
                    .text_color(rgb(T.fg_2)),
            )
            .into_any_element(),
    }
}

/// Opens a native file or folder picker and puts the chosen path in the input.
pub fn browse_path(
    id: &'static str,
    model: &Entity<crate::model::AppModel>,
    input: &Entity<InputState>,
    title: &'static str,
    directory: bool,
) -> impl IntoElement + use<> {
    let model = model.clone();
    let input = input.clone();
    ui::widgets::btn(id, "Browse…")
        .compact()
        .on_click(move |window, cx| {
            let handle = window.window_handle();
            let input = input.clone();
            let on_path = move |path: std::path::PathBuf, cx: &mut App| {
                let _ = handle.update(cx, |_, window, cx| {
                    input.update(cx, |input, cx| {
                        input.set_value(path.display().to_string(), window, cx);
                    });
                });
            };
            if directory {
                crate::model::pick_directory(model.clone(), title, cx, on_path);
            } else {
                crate::model::pick_path(model.clone(), title, None, cx, on_path);
            }
        })
}
