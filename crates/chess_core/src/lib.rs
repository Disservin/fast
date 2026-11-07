//! Chess domain model: game tree with variations, NAGs and comments, PGN in/out,
//! notation helpers and a small opening classifier. No UI dependencies.

pub mod annotation;
pub mod eco;
pub mod notation;
pub mod pgn;
pub mod sample;
pub mod tree;

pub use annotation::{MoveAnnotation, move_times, parse_annotation, strip_commands};
pub use eco::{Opening, classify};
pub use notation::{Notation, display_san, format_pv, nag_symbol, pv_to_san};
pub use pgn::{PgnError, parse_pgn, write_pgn, write_pgn_from};
pub use tree::{GameTree, Node, NodeId, position_hash};

pub use shakmaty::{self, Chess, Color, Move, Position, Role, Square};
