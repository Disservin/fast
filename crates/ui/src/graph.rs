//! Line graphs over the plies of a game: eval (symmetric log scale), nodes, time and depth.
//!
//! Labels are ordinary elements laid over the canvas, so they use the UI fonts.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, canvas,
    div, point, prelude::*, px, quad, relative, rgb, size,
};

use crate::widgets::StyleExt;
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
    /// What the graph shows, for screen readers: "Evaluation graph".
    pub name: &'static str,
    /// Colours for White's and Black's share of the first series: the area below its
    /// line is White's and the area above is Black's, with a dashed line at zero.
    pub sides: Option<(u32, u32)>,
    /// Dots on the first series' line, such as move-quality marks: index and colour.
    pub marks: Vec<(usize, u32)>,
    /// Labels along the bottom edge, such as move numbers: index and text.
    pub x_ticks: Vec<(usize, String)>,
}

/// Move-number labels for a line of `count` plies starting after `first_ply`, about
/// `max` of them at round intervals.
pub fn move_ticks(count: usize, first_ply: u32, max: usize) -> Vec<(usize, String)> {
    if count < 2 || max == 0 {
        return Vec::new();
    }
    let moves = (count as u32 + first_ply).div_ceil(2);
    let step = [1, 2, 5, 10, 20, 25, 50, 100]
        .into_iter()
        .find(|step| moves / step < max as u32)
        .unwrap_or(100);
    (1..)
        .map(|n| n * step)
        .take_while(|number| *number <= moves)
        .filter_map(|number| {
            // White's move `number` is ply 2n-1; index 0 is ply `first_ply + 1`.
            let index = (2 * number - 1).checked_sub(first_ply + 1)? as usize;
            (index < count).then(|| (index, number.to_string()))
        })
        .collect()
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

/// One series as a line, with a filled area down to zero when it asks for one. Gaps in
/// the values break the line unless the series connects them; a lone value is a dot.
fn paint_series(
    window: &mut gpui::Window,
    series: &Series,
    count: usize,
    bounds: Bounds<Pixels>,
    y_of: &dyn Fn(f32) -> f32,
    zero_y: f32,
) {
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
                Bounds::new(point(px(x - 2.), px(y - 2.)), size(px(4.), px(4.))),
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

/// White's share below the line and Black's above it, over each run of known values.
fn paint_sides(
    window: &mut gpui::Window,
    series: &Series,
    count: usize,
    bounds: Bounds<Pixels>,
    y_of: &dyn Fn(f32) -> f32,
    (white, black): (u32, u32),
) {
    let top = f32::from(bounds.origin.y);
    let bottom = top + f32::from(bounds.size.height);
    let mut runs: Vec<Vec<(f32, f32)>> = vec![Vec::new()];
    for (index, value) in series.values.iter().enumerate() {
        match value {
            Some(value) => runs
                .last_mut()
                .expect("non-empty")
                .push((x_of(index, count, bounds), y_of(*value))),
            None => runs.push(Vec::new()),
        }
    }
    for run in runs.iter().filter(|run| run.len() > 1) {
        for (edge, colour) in [(bottom, white), (top, black)] {
            let mut area = vec![point(px(run[0].0), px(edge))];
            area.extend(run.iter().map(|(x, y)| point(px(*x), px(*y))));
            area.push(point(px(run[run.len() - 1].0), px(edge)));
            let mut builder = PathBuilder::fill();
            builder.add_polygon(&area, true);
            if let Ok(path) = builder.build() {
                let mut colour: gpui::Hsla = rgb(colour).into();
                colour.a = 0.85;
                window.paint_path(path, colour);
            }
        }
    }
}

/// A horizontal dashed line across the graph, for the zero line.
fn paint_dashed(window: &mut gpui::Window, bounds: Bounds<Pixels>, y: f32, color: u32) {
    let left = f32::from(bounds.origin.x);
    let right = left + f32::from(bounds.size.width);
    let mut x = left;
    while x < right {
        window.paint_quad(quad(
            Bounds::new(point(px(x), px(y)), size(px(4.0f32.min(right - x)), px(1.))),
            px(0.),
            rgb(color),
            px(0.),
            gpui::transparent_black(),
            Default::default(),
        ));
        x += 8.;
    }
}

/// Coloured dots on the line at the marked indices.
fn paint_marks(
    window: &mut gpui::Window,
    series: &Series,
    marks: &[(usize, u32)],
    count: usize,
    bounds: Bounds<Pixels>,
    y_of: &dyn Fn(f32) -> f32,
) {
    for (index, colour) in marks {
        let Some(Some(value)) = series.values.get(*index) else {
            continue;
        };
        let (x, y) = (x_of(*index, count, bounds), y_of(*value));
        window.paint_quad(quad(
            Bounds::new(point(px(x - 3.5), px(y - 3.5)), size(px(7.), px(7.))),
            px(3.5),
            rgb(*colour),
            px(1.),
            rgb(T.surface),
            Default::default(),
        ));
    }
}

/// The current move: a vertical accent line with a ringed dot on every series.
fn paint_current(
    window: &mut gpui::Window,
    series: &[Series],
    index: usize,
    count: usize,
    bounds: Bounds<Pixels>,
    y_of: &dyn Fn(f32) -> f32,
) {
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
    for series in series {
        if let Some(Some(value)) = series.values.get(index) {
            let y = y_of(*value);
            window.paint_quad(quad(
                Bounds::new(point(px(x - 4.), px(y - 4.)), size(px(8.), px(8.))),
                px(4.),
                rgb(series.color),
                px(1.5),
                rgb(T.surface),
                Default::default(),
            ));
        }
    }
}

/// Pressing on the graph and dragging across it picks the move under the pointer.
fn listen_for_scrub(
    window: &mut gpui::Window,
    on_scrub: &ScrubHandler,
    dragging: &Rc<Cell<bool>>,
    count: usize,
    bounds: Bounds<Pixels>,
) {
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
        name,
        sides,
        marks,
        x_ticks,
    } = props;
    let description = match current {
        Some(index) if count > 0 => crate::i18n::t("{}, {} moves, at move {}")
            .replacen("{}", crate::i18n::t(name), 1)
            .replacen("{}", &count.to_string(), 1)
            .replacen("{}", &(index + 1).to_string(), 1),
        _ => crate::i18n::t(name).to_owned(),
    };
    let bottom_gap = if x_ticks.is_empty() { 6. } else { 20. };
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
        .id(gpui::SharedString::from(format!("graph-{name}")))
        .role(gpui::Role::Image)
        .aria_label(description)
        .relative()
        .w_full()
        .when_some(height, |el, height| el.h(px(height)).flex_none())
        .when(height.is_none(), |el| el.flex_1().min_h(px(80.)))
        .bg(rgb(T.surface))
        .border_as(T.border)
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
                    let zero_y = top + h * (1.0 - scale.zero());
                    if let (Some(colours), Some(first)) = (sides, series.first()) {
                        paint_sides(window, first, count, bounds, &y_of, colours);
                    }
                    for (value, _) in &ticks {
                        line(window, y_of(*value), T.border_strong, 0.6);
                    }
                    if sides.is_some() {
                        paint_dashed(window, bounds, zero_y, T.accent);
                    } else {
                        line(window, zero_y, T.bar, 0.9);
                    }
                    if count == 0 {
                        return;
                    }
                    for series in &series {
                        paint_series(window, series, count, bounds, &y_of, zero_y);
                    }
                    if let Some(first) = series.first() {
                        paint_marks(window, first, &marks, count, bounds, &y_of);
                    }
                    if let Some(index) = current.filter(|index| *index < count) {
                        paint_current(window, &series, index, count, bounds, &y_of);
                    }
                    listen_for_scrub(window, &on_scrub, &dragging, count, bounds);
                },
            )
            .absolute()
            .top(px(6.))
            .bottom(px(bottom_gap))
            .left(px(gutter))
            .right(px(8.)),
        )
        .child(
            div()
                .absolute()
                .top(px(6.))
                .bottom(px(bottom_gap))
                .left_0()
                .when(has_data, |el| el.children(labels)),
        )
        .when(has_data && !x_ticks.is_empty(), |el| {
            let last = count.saturating_sub(1).max(1) as f32;
            el.child(
                div()
                    .absolute()
                    .bottom(px(3.))
                    .h(px(14.))
                    .left(px(gutter))
                    .right(px(8.))
                    .children(x_ticks.into_iter().map(|(index, label)| {
                        div()
                            .absolute()
                            .left(relative(index as f32 / last))
                            .ml(px(-10.))
                            .w(px(20.))
                            .text_center()
                            .line_height(px(14.))
                            .font_family(MONO)
                            .text_as(10., T.muted)
                            .child(label)
                    })),
            )
        })
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
                            .text_as(12., T.muted)
                            .line_height(px(16.))
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
    fn move_ticks_label_whole_moves() {
        // Forty plies from the start: moves 1 to 20, labelled every five.
        let ticks = move_ticks(40, 0, 8);
        assert_eq!(ticks[0], (8, "5".to_owned()));
        assert_eq!(ticks.last(), Some(&(38, "20".to_owned())));
        assert!(move_ticks(1, 0, 8).is_empty());
    }

    #[test]
    fn x_positions_round_trip() {
        let bounds = Bounds::new(point(px(10.), px(0.)), size(px(100.), px(20.)));
        assert_eq!(index_at(x_of(3, 11, bounds), 11, bounds), 3);
        assert_eq!(index_at(-50.0, 11, bounds), 0);
        assert_eq!(index_at(500.0, 11, bounds), 10);
    }
}
