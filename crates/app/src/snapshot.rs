//! The board drawn as a PNG, for sharing a position outside the app.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use chess_core::{
    Chess, Position,
    shakmaty::{File, Rank, Square},
};
use gpui::AssetSource;
use resvg::{tiny_skia, usvg};

use crate::assets::Assets;

/// Side of one square in the image, in pixels.
const SQUARE: u32 = 100;
/// The pieces fill this share of a square, as on the board itself.
const PIECE_SCALE: f32 = 0.74;

/// How the board looks in the image.
pub struct BoardLook {
    pub pieces: ui::PieceSet,
    pub light: u32,
    pub dark: u32,
    pub flipped: bool,
    pub last_move: Option<(Square, Square)>,
}

/// The position as an SVG document, one square `SQUARE` units wide.
pub fn board_svg(pos: &Chess, look: &BoardLook) -> Result<String> {
    let size = SQUARE * 8;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">"#
    );
    let piece_size = SQUARE as f32 * PIECE_SCALE;
    let inset = (SQUARE as f32 - piece_size) / 2.;
    let mut pieces = String::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            let (file, rank) = if look.flipped { (7 - x, y) } else { (x, 7 - y) };
            let square = Square::from_coords(File::new(file), Rank::new(rank));
            let is_light = (file + rank) % 2 == 1;
            let moved = look
                .last_move
                .is_some_and(|(from, to)| from == square || to == square);
            let color = match (is_light, moved) {
                (true, false) => look.light,
                (false, false) => look.dark,
                (true, true) => ui::board::last_move_color(look.light),
                (false, true) => ui::board::last_move_color(look.dark),
            };
            let (left, top) = (x * SQUARE, y * SQUARE);
            svg.push_str(&format!(
                r##"<rect x="{left}" y="{top}" width="{SQUARE}" height="{SQUARE}" fill="#{color:06X}"/>"##
            ));
            let Some(piece) = pos.board().piece_at(square) else {
                continue;
            };
            let side = if piece.color.is_white() { 'w' } else { 'b' };
            let name = format!("{side}{}", piece.role.upper_char());
            let path = ui::PieceSet::IMAGES
                .iter()
                .find(|set| **set == look.pieces)
                .copied()
                .unwrap_or(ui::PieceSet::Classic)
                .asset(piece);
            let bytes = Assets
                .load(&path)?
                .with_context(|| format!("missing {path}"))?;
            let source = std::str::from_utf8(&bytes).context("piece is not UTF-8")?;
            // Piece files reuse short ids (`a`, `b`, …): keep each copy's own.
            let prefix = format!("{name}{square}");
            let renamed = source
                .replace(r#"id=""#, &format!(r#"id="{prefix}"#))
                .replace("url(#", &format!("url(#{prefix}"))
                .replace(r##"href="#"##, &format!(r##"href="#{prefix}"##));
            let nested = place_svg(
                &renamed,
                left as f32 + inset,
                top as f32 + inset,
                piece_size,
            );
            pieces.push_str(&nested);
        }
    }
    svg.push_str(&pieces);
    svg.push_str("</svg>");
    Ok(svg)
}

/// Puts a standalone SVG at a position and size inside another: its own `width`
/// and `height` (in mm, units or absent, depending on the set) give way to ours,
/// and its `viewBox` scales the artwork.
fn place_svg(source: &str, x: f32, y: f32, size: f32) -> String {
    let Some(start) = source.find("<svg") else {
        return String::new();
    };
    let Some(len) = source[start..].find('>') else {
        return String::new();
    };
    let tag = &source[start..start + len];
    let mut kept = String::new();
    let mut rest = &tag[4..];
    // Copy attributes except the outer size.
    while let Some(eq) = rest.find('=') {
        let name_start = rest[..eq]
            .trim_end()
            .rfind(char::is_whitespace)
            .map_or(0, |i| i + 1);
        let name = rest[name_start..eq].trim();
        let after = &rest[eq + 1..];
        let quote = after.chars().next().unwrap_or('"');
        let Some(close) = after[1..].find(quote) else {
            break;
        };
        let value = &after[..close + 2];
        if name != "width" && name != "height" {
            kept.push_str(&format!(" {name}={value}"));
        }
        rest = &after[close + 2..];
    }
    format!(
        r#"<svg x="{x}" y="{y}" width="{size}" height="{size}"{kept}{}"#,
        &source[start + len..]
    )
}

/// The position as PNG bytes.
pub fn board_png(pos: &Chess, look: &BoardLook) -> Result<Vec<u8>> {
    let svg = board_svg(pos, look)?;
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
    let size = SQUARE * 8;
    let mut pixmap = tiny_skia::Pixmap::new(size, size).context("could not allocate image")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().context("could not encode PNG")
}

/// Where board images go: a Fast folder under Pictures, else under home.
pub fn image_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Fast")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_set_renders_at_the_square_size() {
        for pieces in ui::PieceSet::IMAGES {
            let look = BoardLook {
                pieces,
                light: 0xDEE3E6,
                dark: 0x8CA2AD,
                flipped: false,
                last_move: None,
            };
            let svg = board_svg(&Chess::default(), &look).unwrap();
            assert!(!svg.contains("mm\""), "{pieces:?} kept a mm size");
            board_png(&Chess::default(), &look).unwrap();
        }
    }

    #[test]
    fn placed_svgs_take_our_size_and_keep_the_view_box() {
        let placed = place_svg(
            r#"<svg xmlns="x" width="45" height="45" viewBox="0 0 45 45"><g/></svg>"#,
            1.0,
            2.0,
            30.0,
        );
        assert!(
            placed.starts_with(
                r#"<svg x="1" y="2" width="30" height="30" xmlns="x" viewBox="0 0 45 45"><g/>"#
            ),
            "{placed}"
        );
    }

    #[test]
    fn draws_every_piece_once_with_its_own_gradients() {
        let look = BoardLook {
            pieces: ui::PieceSet::Classic,
            light: 0xDEE3E6,
            dark: 0x8CA2AD,
            flipped: true,
            last_move: None,
        };
        let svg = board_svg(&Chess::default(), &look).unwrap();
        assert_eq!(svg.matches("<rect").count(), 64);
        assert_eq!(svg.matches("<svg").count(), 1 + 32);
        let png = board_png(&Chess::default(), &look).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // Set to look at the result.
        if let Ok(out) = std::env::var("FAST_SNAPSHOT_OUT") {
            std::fs::write(out, png).unwrap();
        }
    }
}
