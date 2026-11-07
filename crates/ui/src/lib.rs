//! Shared GPUI views and widgets used by every Fast screen.

pub mod board;
pub mod eval_bar;
pub mod graph;
pub mod scoresheet;
pub mod theme;
pub mod timeline;
pub mod widgets;

pub use board::{Arrow, BoardEvent, BoardPrefs, BoardTheme, BoardView, CoordinateStyle, PieceSet};
pub use theme::{MONO, SANS, T, Theme, UiTheme, apply_component_theme, set_theme};
