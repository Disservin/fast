//! Game timeline: one thin bar per ply showing the eval, doubling as a scrubber.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Window, canvas,
    div, point, prelude::*, px, quad, rgb, size,
};

use crate::{theme::T, widgets::StyleExt as _};

/// Steepness of the saturating eval-to-height curve: small advantages stay
/// readable while a won position simply fills the bar.
const CURVE: f32 = 2.37;

pub type ScrubHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub struct TimelineProps {
    /// White-relative centipawns per ply; `None` when not yet analysed.
    pub evals: Vec<Option<i32>>,
    /// Index of the current ply into `evals`.
    pub current: Option<usize>,
    pub on_scrub: ScrubHandler,
}

/// Height in px of a bar for an eval. Unevaluated plies get a small stub.
fn bar_height(eval: Option<i32>, half: f32) -> f32 {
    let Some(cp) = eval else {
        return 3.0;
    };
    let pawns = cp.abs() as f32 / 100.0;
    ((1.0 - (-pawns * CURVE).exp()) * half).max(3.0)
}

/// Bar slots stay this wide at most, so short games do not stretch across the box.
const MAX_SLOT: f32 = 22.0;

fn slot_width(width: f32, count: usize) -> f32 {
    (width / count.max(1) as f32).min(MAX_SLOT)
}

fn index_at(position: f32, bounds: Bounds<Pixels>, count: usize) -> usize {
    let left = f32::from(bounds.origin.x);
    let slot = slot_width(f32::from(bounds.size.width).max(1.0), count);
    let offset = (position - left).max(0.0);
    ((offset / slot) as usize).min(count.saturating_sub(1))
}

/// The timeline element. Dragging keeps scrubbing even if the pointer leaves it.
pub fn timeline(props: TimelineProps) -> impl IntoElement {
    let TimelineProps {
        evals,
        current,
        on_scrub,
    } = props;
    let dragging = Rc::new(Cell::new(false));
    div()
        .min_h(px(64.))
        .w_full()
        .flex_1()
        .bg(rgb(T.surface))
        .border_as(T.border)
        .rounded(px(6.))
        .px(px(5.))
        .py(px(5.))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _cx| {
                    let count = evals.len();
                    if count == 0 {
                        return;
                    }
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let baseline = height * 0.52;
                    let slot = slot_width(width, count);
                    let gap = (slot * 0.06).clamp(0.5, 1.5);
                    // The largest reach upward and downward.
                    let up = baseline;
                    let down = height - baseline;
                    for (index, &eval) in evals.iter().enumerate() {
                        let is_current = current == Some(index);
                        let positive = eval.is_none_or(|cp| cp >= 0);
                        let reach = if positive { up } else { down };
                        let bar = bar_height(eval, reach);
                        let color = if is_current {
                            T.fg
                        } else if positive {
                            T.bar
                        } else {
                            T.warn
                        };
                        let x = f32::from(bounds.origin.x) + slot * index as f32 + gap / 2.0;
                        let y = f32::from(bounds.origin.y)
                            + if positive { baseline - bar } else { baseline };
                        window.paint_quad(quad(
                            Bounds::new(point(px(x), px(y)), size(px(slot - gap), px(bar))),
                            px(0.),
                            rgb(color),
                            px(0.),
                            gpui::transparent_black(),
                            Default::default(),
                        ));
                    }
                    let scrub = on_scrub.clone();
                    let flag = dragging.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase.bubble() && bounds.contains(&event.position) {
                            flag.set(true);
                            let index = index_at(f32::from(event.position.x), bounds, count);
                            scrub(index, window, cx);
                        }
                    });
                    let scrub = on_scrub.clone();
                    let flag = dragging.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase.bubble() && flag.get() && event.dragging() {
                            let index = index_at(f32::from(event.position.x), bounds, count);
                            scrub(index, window, cx);
                        }
                    });
                    let flag = dragging.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                        if phase.bubble() {
                            flag.set(false);
                        }
                    });
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_heights_scale_and_saturate() {
        assert_eq!(bar_height(None, 20.0), 3.0);
        assert!(bar_height(Some(100), 20.0) > bar_height(Some(42), 20.0));
        assert!((bar_height(Some(-5000), 20.0) - 20.0).abs() < 1e-3);
        assert_eq!(bar_height(Some(1), 20.0), 3.0);
    }

    #[test]
    fn index_mapping_is_clamped() {
        let bounds = Bounds::new(point(px(10.), px(0.)), size(px(100.), px(10.)));
        assert_eq!(index_at(0.0, bounds, 5), 0);
        assert_eq!(index_at(60.0, bounds, 5), 2);
        assert_eq!(index_at(500.0, bounds, 5), 4);
    }
}
