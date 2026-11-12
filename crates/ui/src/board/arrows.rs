//! Arrows on the board: hit-testing them under the pointer and drawing their outline.

use super::*;

/// Whether a point lies on an arrow drawn between two square centres, in board
/// pixels. See [`arrow_distance`].
pub fn arrow_hit(point: (f32, f32), from: (f32, f32), to: (f32, f32), cell: f32) -> bool {
    arrow_distance(point, from, to, cell).is_some()
}

/// How far a point is from an arrow's centre line, as a share of how far a click may
/// be and still count, or `None` when it misses. The band is much wider than the drawn
/// arrow, half a square across along the shaft and wider at the head, and runs a little
/// past the tip, so a click need not be centred. The origin square is left out, since
/// that is where its piece stands.
pub fn arrow_distance(
    point: (f32, f32),
    from: (f32, f32),
    to: (f32, f32),
    cell: f32,
) -> Option<f32> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (px, py) = (point.0 - from.0, point.1 - from.1);
    let along = px * ux + py * uy;
    let across = (px * -uy + py * ux).abs();
    if along < cell * 0.5 || along > length + cell * 0.3 {
        return None;
    }
    let head_len = cell * 0.56;
    let reach = if along >= length - head_len {
        cell * 0.36
    } else {
        cell * 0.25
    };
    (across <= reach).then_some(across / reach)
}

/// The arrow colours an engine can use instead of move quality, in Settings › Engines.
/// Saved engines keep the chosen value, so these stay fixed across palettes.
pub const ENGINE_ARROW_COLOURS: [u32; 5] = [0x4E9DE0, 0xE09B45, 0x50B890, 0xB181D9, 0xE26878];

pub(super) fn arrow_path(
    from: Point<Pixels>,
    to: Point<Pixels>,
    cell: Pixels,
    scale: f32,
) -> Option<gpui::Path<Pixels>> {
    let dx = f32::from(to.x - from.x);
    let dy = f32::from(to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return None;
    }
    let unit = (dx / length, dy / length);
    let normal = (-unit.1, unit.0);
    let cell = f32::from(cell);
    let shaft = cell * 0.095 * scale;
    let head_half = cell * 0.27 * scale;
    let head_len = cell * 0.56 * scale;
    let at = |along: f32, across: f32| {
        point(
            from.x + px(unit.0 * along + normal.0 * across),
            from.y + px(unit.1 * along + normal.1 * across),
        )
    };
    let start = cell * 0.18;
    let neck = length - head_len;
    let mut builder = PathBuilder::fill();
    builder.add_polygon(
        &[
            at(start, -shaft),
            at(neck, -shaft),
            at(neck, -head_half),
            at(length, 0.0),
            at(neck, head_half),
            at(neck, shaft),
            at(start, shaft),
        ],
        true,
    );
    builder.build().ok()
}
