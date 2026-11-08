pub mod analyse;
pub mod database;
pub mod play;
pub mod settings;
pub mod test_positions;
pub mod tournament;
pub mod train;

use gpui::{AnyElement, ElementId, IntoElement, Pixels, Window, div, prelude::*, px};
use ui::widgets::h_flex;

/// The board edge for a screen with a side column on each side of the board.
pub fn side_columns_board_size(window: &Window) -> Pixels {
    let viewport = window.viewport_size();
    ui::board::snap_board_size(
        (viewport.height - px(362.))
            .min(viewport.width - px(720.))
            .max(px(320.)),
        window.scale_factor(),
    )
}

/// A fixed left column, the board column, and a right column taking the rest; the
/// center and right columns scroll on their own.
pub fn three_columns(
    left: impl IntoElement,
    center_id: impl Into<ElementId>,
    center: impl IntoElement,
    right_id: impl Into<ElementId>,
    right: impl IntoElement,
) -> AnyElement {
    h_flex()
        .items_start()
        .size_full()
        .child(left)
        .child(
            div()
                .id(center_id)
                .px(px(28.))
                .h_full()
                .flex_none()
                .overflow_y_scroll()
                .child(center),
        )
        .child(
            div()
                .id(right_id)
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .pt(px(24.))
                .pr(px(24.))
                .pb(px(16.))
                .child(right),
        )
        .into_any_element()
}
