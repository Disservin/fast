//! Chess domain model: game tree with variations, NAGs and comments, PGN in/out,
//! notation helpers and a small opening classifier. No UI dependencies.

pub mod annotation;
pub mod chess960;
pub mod eco;
pub mod epd;
pub mod material;
pub mod notation;
pub mod pattern;
pub mod pgn;
pub mod rules;
pub mod sample;
pub mod tree;

pub use annotation::{
    MoveAnnotation, eval_command, move_times, parse_annotation, parse_move_annotation,
    strip_commands, with_comment_text, with_eval_command,
};
pub use chess960::{Variant, back_rank, chess960_number, chess960_position, fen_variant};
pub use eco::{Opening, classify};
pub use epd::{EpdLine, EpdPosition, parse_epd};
pub use material::{MaterialBalance, material_balance};
pub use notation::{Notation, display_san, format_pv, nag_symbol, pv_to_san};
pub use pattern::PositionPattern;
pub use pgn::{PgnError, parse_pgn, write_pgn, write_pgn_from};
pub use rules::{GameEnd, game_end};
pub use tree::{GameTree, Node, NodeId, position_hash};

pub use shakmaty::{self, Chess, Color, Move, Position, Role, Square};
