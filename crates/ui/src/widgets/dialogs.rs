//! Modal overlays and dialogs with a header and close button.

use super::*;

/// A modal surface that closes on Escape or a backdrop click.
#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    content: AnyElement,
    on_close: Rc<dyn Fn(&mut App)>,
}

/// Wraps dialog content with shared focus and dismissal behavior.
pub fn modal(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    on_close: impl Fn(&mut App) + 'static,
) -> Modal {
    Modal {
        id: id.into(),
        content: content.into_any_element(),
        on_close: Rc::new(on_close),
    }
}

/// A dialog over the screen: a `surface` panel `width` wide with a 44px header holding
/// `title` and a close button, then `body`. It closes on Escape, the close button or a
/// click on the backdrop, all of which call `on_close`.
pub fn dialog(
    id: impl Into<ElementId>,
    title: impl AsRef<str>,
    width: f32,
    body: impl IntoElement,
    on_close: impl Fn(&mut App) + 'static,
) -> Modal {
    let id = id.into();
    let child =
        |name: &'static str| ElementId::NamedChild(std::sync::Arc::new(id.clone()), name.into());
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let close_button = on_close.clone();
    let panel = v_flex()
        .id(child("panel"))
        .w(px(width))
        .max_w_full()
        .max_h_full()
        .overflow_y_scroll()
        .rounded(px(6.))
        .bg(rgb(T.surface))
        .border_1()
        .border_color(rgb(T.border_strong))
        .child(
            panel_header(title).child(
                icon_btn(child("close"), "icons/x.svg")
                    .mini()
                    .tooltip("Close")
                    .on_click(move |_, cx| close_button(cx)),
            ),
        )
        .child(body);
    Modal {
        id,
        content: panel.into_any_element(),
        on_close,
    }
}

impl RenderOnce for Modal {
    /// Focuses a newly opened dialog so Escape also works before editing a field.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |window, cx| {
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                focus
            })
            .read(cx)
            .clone();
        let close = self.on_close;
        let close_key = close.clone();
        div()
            .id(self.id)
            .track_focus(&focus)
            .key_context("Dialog")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(gpui::rgba(0x0000_0099))
            .capture_key_down(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    close_key(cx);
                    cx.stop_propagation();
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| close(cx))
            .child(
                div()
                    .max_w_full()
                    .max_h_full()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.content),
            )
    }
}
