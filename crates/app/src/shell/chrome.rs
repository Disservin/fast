//! The window frame on systems where Fast draws its own: dragging, window buttons and resize edges.

use super::*;

/// Lets the top bar move the window when the app draws its own decorations.
pub(super) fn drag_region() -> impl IntoElement {
    let region = div().id("window-drag").absolute().inset_0();
    if DRAWS_TITLE_BAR {
        // Windows treats this as its caption: dragging, double-click to maximize,
        // snapping and the system menu all work natively.
        return region.window_control_area(WindowControlArea::Drag);
    }
    if MAC_TITLE_BAR {
        // A double click does what the user chose in System Settings: zoom or minimize.
        return region.on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        });
    }
    region
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |event, window, _| {
            window.show_window_menu(event.position);
        })
}

/// On Windows, marks a control so the system handles it, as with its own caption
/// buttons (hover state, Snap Layouts on maximize).
pub(super) fn system_control(area: WindowControlArea, control: Btn) -> impl IntoElement {
    div()
        .when(DRAWS_TITLE_BAR, |el| el.window_control_area(area))
        .child(control)
}

/// Minimize, maximize and close, for systems that leave decorations to the app.
pub(super) fn window_controls(window: &Window) -> impl IntoElement {
    let available = window.window_controls();
    let maximized = window.is_maximized();
    let control = |id: &'static str, icon: &'static str, tip: &'static str| {
        icon_btn(id, icon)
            .kind(BtnKind::Ghost)
            .w(WINDOW_CONTROL_W)
            .h(32.)
            .tooltip(tip)
    };
    h_flex()
        .gap(px(2.))
        .when(available.minimize || DRAWS_TITLE_BAR, |row| {
            row.child(system_control(
                WindowControlArea::Min,
                control(
                    "window-minimize",
                    "icons/minus.svg",
                    ui::i18n::t("Minimize"),
                )
                .on_click(|window, _| window.minimize_window()),
            ))
        })
        .when(available.maximize || DRAWS_TITLE_BAR, |row| {
            row.child(system_control(
                WindowControlArea::Max,
                control(
                    "window-maximize",
                    "icons/square.svg",
                    if maximized {
                        ui::i18n::t("Restore")
                    } else {
                        ui::i18n::t("Maximize")
                    },
                )
                .on_click(|window, _| window.zoom_window()),
            ))
        })
        .child(system_control(
            WindowControlArea::Close,
            control("window-close", "icons/x.svg", ui::i18n::t("Close"))
                .on_click(|window, _| window.remove_window()),
        ))
}

/// Invisible strips along the window edges for resizing.
pub(super) fn resize_handles() -> Vec<gpui::Stateful<gpui::Div>> {
    const EDGE: Pixels = px(5.);
    const CORNER: Pixels = px(10.);
    let handle = |index: u32, edge: ResizeEdge, cursor: CursorStyle| {
        div()
            .id(("window-resize", index))
            .absolute()
            .occlude()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
    };
    vec![
        handle(0, ResizeEdge::Top, CursorStyle::ResizeUpDown)
            .top_0()
            .left(CORNER)
            .right(CORNER)
            .h(EDGE),
        handle(1, ResizeEdge::Bottom, CursorStyle::ResizeUpDown)
            .bottom_0()
            .left(CORNER)
            .right(CORNER)
            .h(EDGE),
        handle(2, ResizeEdge::Left, CursorStyle::ResizeLeftRight)
            .left_0()
            .top(CORNER)
            .bottom(CORNER)
            .w(EDGE),
        handle(3, ResizeEdge::Right, CursorStyle::ResizeLeftRight)
            .right_0()
            .top(CORNER)
            .bottom(CORNER)
            .w(EDGE),
        handle(4, ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight)
            .top_0()
            .left_0()
            .size(CORNER),
        handle(5, ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft)
            .top_0()
            .right_0()
            .size(CORNER),
        handle(
            6,
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
        )
        .bottom_0()
        .left_0()
        .size(CORNER),
        handle(
            7,
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
        )
        .bottom_0()
        .right_0()
        .size(CORNER),
    ]
}
