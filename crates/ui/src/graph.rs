//! Line graphs over the plies of a game: eval (symmetric log scale), nodes, time and depth.
//!
//! Labels are ordinary elements laid over the canvas, so they use the UI fonts.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, canvas,
    div, point, prelude::*, px, quad, relative, rgb, size,
};

use crate::widgets::TextExt;
use crate::{
    theme::{MONO, T},
    timeline::ScrubHandler,
};

/// How values map to height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// `0..=max`, zero at the bottom.
    Linear { max: f32 },
    /// `-max..=max` on a symmetric log curve with zero in the middle: a pawn of
    /// advantage is clearly visible, and a won position does not flatten the rest.
    SymLog { max: f32, unit: f32 },
}

impl Scale {
    /// Height as a fraction of the graph, 0 at the bottom and 1 at the top.
    pub fn fraction(self, value: f32) -> f32 {
        match self {
            Self::Linear { max } => (value / max.max(f32::EPSILON)).clamp(0.0, 1.0),
            Self::SymLog { max, unit } => {
                let curve = |v: f32| (1.0 + v.abs() / unit).ln();
                let reach = curve(max).max(f32::EPSILON);
                let clamped = value.clamp(-max, max);
                0.5 + 0.5 * clamped.signum() * curve(clamped) / reach
            }
        }
    }

    fn zero(self) -> f32 {
        match self {
            Self::Linear { .. } => 0.0,
            Self::SymLog { .. } => 0.5,
        }
    }
}

pub struct Series {
    pub values: Vec<Option<f32>>,
    pub color: u32,
    /// Shade the area between the line and zero (White above, Black below).
    pub fill: bool,
    /// Join points across missing values instead of breaking the line, for series
    /// that only have a value every other ply (one side's clock).
    pub connect: bool,
}

/// Data and sizing for an interactive game graph.
pub struct GraphProps {
    pub series: Vec<Series>,
    /// Plies on the x axis.
    pub count: usize,
    pub current: Option<usize>,
    pub scale: Scale,
    /// Horizontal guides: value and label.
    pub ticks: Vec<(f32, String)>,
    /// Fixed height, or fill the available space with an 80px minimum.
    pub height: Option<f32>,
    pub on_scrub: ScrubHandler,
    /// Shown when no series has a value yet.
    pub empty: &'static str,
}

/// Eval ticks for the symmetric log scale, in pawns.
pub fn eval_ticks() -> Vec<(f32, String)> {
    [-1000.0, -300.0, -100.0, 0.0, 100.0, 300.0, 1000.0]
        .into_iter()
        .map(|cp: f32| {
            let label = if cp == 0.0 {
                "0".to_owned()
            } else {
                format!("{:+}", cp / 100.0)
            };
            (cp, label)
        })
        .collect()
}

/// A few round numbers up to `max` for a linear axis.
pub fn linear_ticks(max: f32, unit: &str) -> Vec<(f32, String)> {
    if max <= 0.0 {
        return Vec::new();
    }
    let raw = max / 3.0;
    let magnitude = 10f32.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|step| *step >= raw)
        .unwrap_or(raw);
    (1..)
        .map(|i| i as f32 * step)
        .take_while(|value| *value <= max)
        .map(|value| (value, format!("{}{unit}", trim_number(value))))
        .collect()
}

/// Fits node counts to about five round, whole-number intervals with a labelled ceiling.
pub fn nodes_axis(max: f32) -> (Scale, Vec<(f32, String)>) {
    let max = if max.is_finite() { max.max(1.0) } else { 1.0 };
    let raw = max / 5.0;
    let magnitude = 10f32.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|step| *step >= raw)
        .unwrap_or(raw)
        .max(1.0);
    let intervals = (max / step).ceil() as usize;
    let ceiling = intervals as f32 * step;
    let ticks = (0..=intervals)
        .map(|index| {
            let value = index as f32 * step;
            (value, format!("{value:.0}"))
        })
        .collect();
    (Scale::Linear { max: ceiling }, ticks)
}

/// Keeps full axis labels clear of the plotted lines at the graph's mono font size.
fn label_gutter(ticks: &[(f32, String)]) -> f32 {
    let characters = ticks
        .iter()
        .map(|(_, label)| label.len())
        .max()
        .unwrap_or(0);
    (characters as f32 * 6.0 + 12.0).max(30.0)
}

fn trim_number(value: f32) -> String {
    if value.fract().abs() < 1e-3 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn x_of(index: usize, count: usize, bounds: Bounds<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width);
    let left = f32::from(bounds.origin.x);
    if count <= 1 {
        return left + width / 2.0;
    }
    left + width * index as f32 / (count - 1) as f32
}

fn index_at(x: f32, count: usize, bounds: Bounds<Pixels>) -> usize {
    let width = f32::from(bounds.size.width).max(1.0);
    let offset = (x - f32::from(bounds.origin.x)).clamp(0.0, width);
    if count <= 1 {
        return 0;
    }
    ((offset / width * (count - 1) as f32).round() as usize).min(count - 1)
}

/// Draws a resizable game graph that supports scrubbing through moves.
pub fn graph(props: GraphProps) -> impl IntoElement {
    let GraphProps {
        series,
        count,
        current,
        scale,
        ticks,
        height,
        on_scrub,
        empty,
    } = props;
    let has_data = series
        .iter()
        .any(|series| series.values.iter().any(Option::is_some));
    let gutter = label_gutter(&ticks);
    let labels: Vec<_> = ticks
        .iter()
        .map(|(value, label)| {
            let top = 1.0 - scale.fraction(*value);
            div()
                .absolute()
                .left(px(4.))
                .top(relative(top))
                .mt(px(-7.))
                .whitespace_nowrap()
                .line_height(px(14.))
                .font_family(MONO)
                .text_as(10., T.muted)
                .child(label.clone())
        })
        .collect();
    let dragging = Rc::new(Cell::new(false));
    div()
        .relative()
        .w_full()
        .when_some(height, |el, height| el.h(px(height)).flex_none())
        .when(height.is_none(), |el| el.flex_1().min_h(px(80.)))
        .bg(rgb(T.surface))
        .border_1()
        .border_color(rgb(T.border))
        .rounded(px(6.))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // An empty graph shows only its message.
                    if !has_data {
                        return;
                    }
                    let top = f32::from(bounds.origin.y);
                    let h = f32::from(bounds.size.height);
                    let y_of = |value: f32| top + h * (1.0 - scale.fraction(value));
                    let line = |window: &mut gpui::Window, y: f32, color: u32, alpha: f32| {
                        let mut color: gpui::Hsla = rgb(color).into();
                        color.a = alpha;
                        window.paint_quad(quad(
                            Bounds::new(
                                point(bounds.origin.x, px(y)),
                                size(bounds.size.width, px(1.)),
                            ),
                            px(0.),
                            color,
                            px(0.),
                            gpui::transparent_black(),
                            Default::default(),
                        ));
                    };
                    for (value, _) in &ticks {
                        line(window, y_of(*value), T.border_strong, 0.6);
                    }
                    let zero_y = top + h * (1.0 - scale.zero());
                    line(window, zero_y, T.bar, 0.9);
                    if count == 0 {
                        return;
                    }
                    for series in &series {
                        // Runs of known values; gaps break the line.
                        let mut runs: Vec<Vec<(f32, f32)>> = vec![Vec::new()];
                        for (index, value) in series.values.iter().enumerate() {
                            match value {
                                Some(value) => runs
                                    .last_mut()
                                    .expect("non-empty")
                                    .push((x_of(index, count, bounds), y_of(*value))),
                                None if !series.connect => runs.push(Vec::new()),
                                None => {}
                            }
                        }
                        for run in runs.iter().filter(|run| !run.is_empty()) {
                            if series.fill && run.len() > 1 {
                                let mut area = vec![point(px(run[0].0), px(zero_y))];
                                area.extend(run.iter().map(|(x, y)| point(px(*x), px(*y))));
                                area.push(point(px(run[run.len() - 1].0), px(zero_y)));
                                let mut builder = PathBuilder::fill();
                                builder.add_polygon(&area, true);
                                if let Ok(path) = builder.build() {
                                    let mut color: gpui::Hsla = rgb(series.color).into();
                                    color.a = 0.16;
                                    window.paint_path(path, color);
                                }
                            }
                            if run.len() == 1 {
                                let (x, y) = run[0];
                                window.paint_quad(quad(
                                    Bounds::new(
                                        point(px(x - 2.), px(y - 2.)),
                                        size(px(4.), px(4.)),
                                    ),
                                    px(2.),
                                    rgb(series.color),
                                    px(0.),
                                    gpui::transparent_black(),
                                    Default::default(),
                                ));
                                continue;
                            }
                            let mut builder = PathBuilder::stroke(px(2.));
                            builder.move_to(point(px(run[0].0), px(run[0].1)));
                            for (x, y) in &run[1..] {
                                builder.line_to(point(px(*x), px(*y)));
                            }
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, rgb(series.color));
                            }
                        }
                    }
                    if let Some(index) = current.filter(|index| *index < count) {
                        let x = x_of(index, count, bounds);
                        let mut color: gpui::Hsla = rgb(T.accent).into();
                        color.a = 0.85;
                        window.paint_quad(quad(
                            Bounds::new(
                                point(px(x - 1.), bounds.origin.y),
                                size(px(2.), bounds.size.height),
                            ),
                            px(0.),
                            color,
                            px(0.),
                            gpui::transparent_black(),
                            Default::default(),
                        ));
                        for series in &series {
                            if let Some(Some(value)) = series.values.get(index) {
                                let y = y_of(*value);
                                window.paint_quad(quad(
                                    Bounds::new(
                                        point(px(x - 4.), px(y - 4.)),
                                        size(px(8.), px(8.)),
                                    ),
                                    px(4.),
                                    rgb(series.color),
                                    px(1.5),
                                    rgb(T.surface),
                                    Default::default(),
                                ));
                            }
                        }
                    }
                    let scrub = on_scrub.clone();
                    let flag = dragging.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase.bubble() && bounds.contains(&event.position) {
                            flag.set(true);
                            scrub(
                                index_at(f32::from(event.position.x), count, bounds),
                                window,
                                cx,
                            );
                        }
                    });
                    let scrub = on_scrub.clone();
                    let flag = dragging.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase.bubble() && flag.get() && event.dragging() {
                            scrub(
                                index_at(f32::from(event.position.x), count, bounds),
                                window,
                                cx,
                            );
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
            .absolute()
            .top(px(6.))
            .bottom(px(6.))
            .left(px(gutter))
            .right(px(8.)),
        )
        .child(
            div()
                .absolute()
                .top(px(6.))
                .bottom(px(6.))
                .left_0()
                .when(has_data, |el| el.children(labels)),
        )
        .when(!has_data, |el| {
            el.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(12.))
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(rgb(T.muted))
                            .child(empty),
                    ),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symlog_is_symmetric_and_saturates() {
        let scale = Scale::SymLog {
            max: 2000.0,
            unit: 100.0,
        };
        assert!((scale.fraction(0.0) - 0.5).abs() < 1e-6);
        let up = scale.fraction(150.0);
        let down = scale.fraction(-150.0);
        assert!((up - 0.5 - (0.5 - down)).abs() < 1e-6);
        assert!((scale.fraction(5000.0) - 1.0).abs() < 1e-6);
        assert!(scale.fraction(-5000.0).abs() < 1e-6);
        // A pawn takes a visible share of the half height, unlike on a linear scale.
        assert!(scale.fraction(100.0) - 0.5 > 0.08);
        assert!(scale.fraction(200.0) > scale.fraction(100.0));
    }

    #[test]
    fn linear_ticks_are_round() {
        let ticks: Vec<f32> = linear_ticks(40.0, "s").iter().map(|t| t.0).collect();
        assert_eq!(ticks, vec![20.0, 40.0]);
        assert_eq!(linear_ticks(0.0, "").len(), 0);
        assert_eq!(linear_ticks(30.0, "")[0].1, "10");
    }

    /// Node guides stay readable as search sizes grow from single nodes to billions.
    #[test]
    fn nodes_axis_uses_round_intervals_and_full_labels() {
        for (maximum, step, ceiling) in [
            (0.0, 1.0, 1.0),
            (3.0, 1.0, 3.0),
            (900_000.0, 200_000.0, 1_000_000.0),
            (10_000_000.0, 2_000_000.0, 10_000_000.0),
            (10_100_000.0, 5_000_000.0, 15_000_000.0),
            (1_200_000_000.0, 500_000_000.0, 1_500_000_000.0),
        ] {
            let (scale, ticks) = nodes_axis(maximum);
            assert_eq!(scale, Scale::Linear { max: ceiling });
            assert_eq!(ticks.first(), Some(&(0.0, "0".to_owned())));
            assert_eq!(ticks[1].0, step);
            assert_eq!(ticks.last(), Some(&(ceiling, format!("{ceiling:.0}"))));
            assert!(ticks.len() <= 6);
            assert!(
                ticks
                    .iter()
                    .all(|(_, label)| label.chars().all(|c| c.is_ascii_digit()))
            );
            assert!(scale.fraction(maximum) <= 1.0);
        }
    }

    /// Full node labels reserve enough horizontal room before the plotting area.
    #[test]
    fn axis_gutter_fits_large_node_counts() {
        let (_, ticks) = nodes_axis(10_000_000.0);
        assert_eq!(label_gutter(&ticks), 60.0);
        assert_eq!(label_gutter(&eval_ticks()), 30.0);
    }

    #[test]
    fn x_positions_round_trip() {
        let bounds = Bounds::new(point(px(10.), px(0.)), size(px(100.), px(20.)));
        assert_eq!(index_at(x_of(3, 11, bounds), 11, bounds), 3);
        assert_eq!(index_at(-50.0, 11, bounds), 0);
        assert_eq!(index_at(500.0, 11, bounds), 10);
    }
}
