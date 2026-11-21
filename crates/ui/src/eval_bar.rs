//! Retargetable evaluation-bar animation shared by analysis and tournament boards.

use crate::T;
use gpui::{
    App, ElementId, IntoElement, Pixels, RenderOnce, StatefulInteractiveElement, Window, div,
    prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

const TRANSITION: Duration = Duration::from_millis(650);

struct Transition {
    from: f32,
    target: f32,
    started: Instant,
}

impl Transition {
    fn sample(&self, now: Instant) -> f32 {
        let progress =
            (now.duration_since(self.started).as_secs_f32() / TRANSITION.as_secs_f32()).min(1.);
        self.from + (self.target - self.from) * (1. - (1. - progress).powi(3))
    }
}

#[derive(IntoElement)]
pub struct EvalBar {
    id: ElementId,
    share: f32,
    edge: Pixels,
    flipped: bool,
    mid_line: bool,
    label: Option<gpui::SharedString>,
}

impl EvalBar {
    pub fn new(id: impl Into<ElementId>, share: f32, edge: Pixels) -> Self {
        Self {
            id: id.into(),
            share: share.clamp(0., 1.),
            edge,
            flipped: false,
            mid_line: false,
            label: None,
        }
    }

    pub fn flipped(mut self, flipped: bool) -> Self {
        self.flipped = flipped;
        self
    }

    /// What screen readers say for the bar, such as "Evaluation +0.42".
    pub fn label(mut self, label: impl Into<gpui::SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Marks the middle, where the position is equal, with a thin `accent` line.
    pub fn mid_line(mut self) -> Self {
        self.mid_line = true;
        self
    }
}

impl RenderOnce for EvalBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let now = Instant::now();
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| Transition {
            from: self.share,
            target: self.share,
            started: now,
        });
        let (share, animating) = state.update(cx, |state, _| {
            if state.target != self.share {
                state.from = state.sample(now);
                state.target = self.share;
                state.started = now;
            }
            (
                state.sample(now),
                now.duration_since(state.started) < TRANSITION && state.from != state.target,
            )
        });
        if animating {
            window.on_next_frame(move |_, cx| state.update(cx, |_, cx| cx.notify()));
        }
        let width = px(8.);
        let radius = width / 2.;
        div()
            .id(self.id)
            .role(gpui::Role::Image)
            .when_some(self.label, |bar, label| bar.aria_label(label))
            .relative()
            .w(width)
            .h(self.edge)
            .flex_none()
            .rounded(radius)
            .bg(rgb(T.tour_tile_border))
            .child(
                // Clip a full rounded fill so only the moving boundary stays flat.
                div()
                    .absolute()
                    .left(px(0.))
                    .w_full()
                    .h(self.edge * share)
                    .overflow_hidden()
                    .when(self.flipped, |el| el.top(px(0.)))
                    .when(!self.flipped, |el| el.bottom(px(0.)))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .w_full()
                            .h(self.edge)
                            .when(self.flipped, |el| el.top(px(0.)))
                            .when(!self.flipped, |el| el.bottom(px(0.)))
                            .rounded(radius)
                            .bg(rgb(T.fg)),
                    ),
            )
            .when(self.mid_line, |bar| {
                bar.child(
                    div()
                        .absolute()
                        .left(px(-2.))
                        .w(width + px(4.))
                        .h(px(2.))
                        .top(self.edge / 2. - px(1.))
                        .bg(rgb(T.accent)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_moves_smoothly_and_can_retarget_without_jumping() {
        let now = Instant::now();
        let transition = Transition {
            from: 0.2,
            target: 0.8,
            started: now,
        };
        assert_eq!(transition.sample(now), 0.2);
        let midway = transition.sample(now + TRANSITION / 2);
        assert!(midway > 0.2 && midway < 0.8);
        assert!((transition.sample(now + TRANSITION) - 0.8).abs() < 0.0001);
        let retarget = Transition {
            from: midway,
            target: 0.1,
            started: now + TRANSITION / 2,
        };
        assert_eq!(retarget.sample(now + TRANSITION / 2), midway);
    }
}
