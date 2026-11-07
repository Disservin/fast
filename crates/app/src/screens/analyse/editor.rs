//! Edit position: place pieces freely, pick the side to move and castling, then play on.

use chess_core::{
    Chess, Color, GameTree, Position, Role,
    shakmaty::{Bitboard, Board, CastlingMode, FromSetup, Piece, Setup, Square},
};
use gpui::{Context, IntoElement, MouseButton, SharedString, div, img, prelude::*, px, rgb};
use ui::{
    T,
    widgets::{btn, check, h_flex, icon_btn, v_flex},
};

use super::AnalyseScreen;
use crate::controls::text_input;

/// Castling rights by the rook each one needs: White O-O, O-O-O, Black O-O, O-O-O.
const CASTLING: [(Square, &str); 4] = [
    (Square::H1, "O-O"),
    (Square::A1, "O-O-O"),
    (Square::H8, "O-O"),
    (Square::A8, "O-O-O"),
];

const ROLES: [Role; 6] = [
    Role::King,
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

/// A position being set up. The board may be illegal until it is applied.
#[derive(Clone, Debug)]
pub struct PositionEditor {
    pub board: Board,
    pub turn: Color,
    pub castling: [bool; 4],
    /// The piece a click places; `None` erases.
    pub brush: Option<Piece>,
}

impl PositionEditor {
    pub fn from_position(pos: &Chess) -> Self {
        let rights = pos.castles().castling_rights();
        Self {
            board: pos.board().clone(),
            turn: pos.turn(),
            castling: CASTLING.map(|(rook, _)| rights.contains(rook)),
            brush: Some(Piece {
                color: Color::White,
                role: Role::Pawn,
            }),
        }
    }

    /// Places the brush, or erases. Clicking a piece with the same piece removes it.
    pub fn paint(&mut self, square: Square, erase: bool) {
        match self.brush.filter(|_| !erase) {
            Some(piece) if self.board.piece_at(square) != Some(piece) => {
                self.board.set_piece_at(square, piece);
            }
            _ => {
                self.board.discard_piece_at(square);
            }
        }
    }

    pub fn setup(&self) -> Setup {
        let mut setup = Setup::empty();
        setup.board = self.board.clone();
        setup.turn = self.turn;
        setup.castling_rights = CASTLING
            .iter()
            .zip(self.castling)
            .filter(|(_, on)| *on)
            .fold(Bitboard::EMPTY, |rights, ((rook, _), _)| {
                rights | Bitboard::from(*rook)
            });
        setup
    }

    /// The legal position, or why there is none. Castling that the pieces no longer
    /// allow is dropped rather than refused.
    pub fn position(&self) -> Result<Chess, String> {
        let checked = match Chess::from_setup(self.setup(), CastlingMode::Standard) {
            Ok(pos) => Ok(pos),
            Err(err) => err.ignore_invalid_castling_rights(),
        };
        checked.map_err(|err| {
            let text = err.to_string();
            text.strip_prefix("illegal position: ")
                .map_or(text.clone(), |reason| {
                    let mut reason = reason.to_owned();
                    if let Some(first) = reason.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    reason
                })
        })
    }
}

fn piece_path(piece: Piece) -> SharedString {
    let side = if piece.color.is_white() { 'w' } else { 'b' };
    format!("pieces/merida/{side}{}.svg", piece.role.upper_char()).into()
}

impl AnalyseScreen {
    /// Opens the editor on the position on the board, or closes it without applying.
    pub(super) fn toggle_editor(&mut self, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            self.close_editor(cx);
            return;
        }
        let editor = PositionEditor::from_position(&self.analysed_pos());
        let board = editor.board.clone();
        self.editor = Some(editor);
        self.board
            .update(cx, |view, cx| view.set_editing(Some(board), cx));
        cx.notify();
    }

    pub(super) fn close_editor(&mut self, cx: &mut Context<Self>) {
        if self.editor.take().is_some() {
            self.board.update(cx, |view, cx| view.set_editing(None, cx));
            cx.notify();
        }
    }

    /// Changes the edited position and shows it.
    pub(super) fn edit_position(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut PositionEditor),
    ) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        change(editor);
        let board = editor.board.clone();
        self.board
            .update(cx, |view, cx| view.set_editing(Some(board), cx));
        cx.notify();
    }

    /// Starts a new game from the edited position, if it is legal.
    fn apply_editor(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        match editor.position() {
            Ok(pos) => {
                let fen = GameTree::fen_of(&pos);
                self.close_editor(cx);
                self.load_fen(&fen, window, cx);
            }
            Err(reason) => self.toast(format!("Can't use this position: {reason}"), cx),
        }
    }

    pub(super) fn render_editor(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let Some(editor) = self.editor.clone() else {
            return div().into_any_element();
        };
        let cell = 40.;
        // Light squares keep both colours of piece readable, as on the board.
        let (light, _) = self.board.read(cx).prefs().theme.colors();
        let swatch = |id: (&'static str, u32), selected: bool| {
            div()
                .id(id)
                .size(px(cell))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .border_2()
                .border_color(rgb(if selected { T.arrow } else { T.surface }))
                .bg(rgb(if selected { T.last_move_light } else { light }))
                .hover(|style| style.bg(rgb(T.last_move_light)))
        };
        let palette = |color: Color| {
            let mut row = h_flex().gap(px(6.));
            for role in ROLES {
                let piece = Piece { color, role };
                let selected = editor.brush == Some(piece);
                let pick = this.clone();
                row = row.child(
                    swatch(
                        (
                            if color.is_white() {
                                "brush-w"
                            } else {
                                "brush-b"
                            },
                            role as u32,
                        ),
                        selected,
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        pick.update(cx, |screen, cx| {
                            screen.edit_position(cx, |editor| editor.brush = Some(piece));
                        });
                    })
                    .child(img(piece_path(piece)).size(px(cell * 0.8))),
                );
            }
            row
        };
        let erase_this = this.clone();
        let eraser = icon_btn("brush-erase", "icons/x.svg")
            .w(cell)
            .h(cell)
            .selected(editor.brush.is_none())
            .tooltip("Erase (or right-click a square)")
            .on_click(move |_, cx| {
                erase_this.update(cx, |screen, cx| {
                    screen.edit_position(cx, |editor| editor.brush = None);
                });
            });
        let side = |color: Color| {
            let label = if color.is_white() { "White" } else { "Black" };
            let turn_this = this.clone();
            btn(("editor-turn", color as u32), label)
                .h(30.)
                .pad(10.)
                .text(13.)
                .selected(editor.turn == color)
                .on_click(move |_, cx| {
                    turn_this.update(cx, |screen, cx| {
                        screen.edit_position(cx, |editor| editor.turn = color);
                    });
                })
        };
        // One row per side: its name, then O-O and O-O-O.
        let castle = |index: usize| {
            let castle_this = this.clone();
            div().w(px(84.)).flex_none().child(
                check(
                    ("editor-castle", index),
                    editor.castling[index],
                    CASTLING[index].1,
                )
                .text(13.)
                .on_change(move |value, _, cx| {
                    castle_this.update(cx, |screen, cx| {
                        screen.edit_position(cx, |editor| editor.castling[index] = value);
                    });
                }),
            )
        };
        let castle_row = |label: &'static str, first: usize| {
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(48.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(label),
                )
                .child(castle(first))
                .child(castle(first + 1))
        };
        let castling = v_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child("Castling"),
            )
            .child(castle_row("White", 0))
            .child(castle_row("Black", 2));
        let status = match editor.position() {
            Ok(pos) => (GameTree::fen_of(&pos), T.muted),
            Err(reason) => (reason, T.warn),
        };
        let start_this = this.clone();
        let clear_this = this.clone();
        let cancel_this = this.clone();
        let done_this = this.clone();
        let tool = |id: &'static str, label: &'static str| btn(id, label).h(30.).pad(10.).text(13.);
        v_flex()
            .mt(px(12.))
            .p(px(14.))
            .gap(px(12.))
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(T.border))
            .rounded(px(8.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_start()
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(palette(Color::White))
                            .child(palette(Color::Black)),
                    )
                    .child(eraser),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .mr(px(4.))
                            .child("To move"),
                    )
                    .child(side(Color::White))
                    .child(side(Color::Black)),
            )
            .child(castling)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(status.1))
                    .font_family(ui::MONO)
                    .child(status.0),
            )
            .child(div().child(text_input(&self.fen_input)))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        tool("editor-start", "Start position").on_click(move |_, cx| {
                            start_this.update(cx, |screen, cx| {
                                screen.edit_position(cx, |editor| {
                                    let brush = editor.brush;
                                    *editor = PositionEditor::from_position(&Chess::default());
                                    editor.brush = brush;
                                });
                            });
                        }),
                    )
                    .child(tool("editor-clear", "Clear").on_click(move |_, cx| {
                        clear_this.update(cx, |screen, cx| {
                            screen.edit_position(cx, |editor| {
                                editor.board = Board::empty();
                                editor.castling = [false; 4];
                            });
                        });
                    }))
                    .child(div().flex_1())
                    .child(tool("editor-cancel", "Cancel").on_click(move |_, cx| {
                        cancel_this.update(cx, |screen, cx| screen.close_editor(cx));
                    }))
                    .child(
                        tool("editor-done", "Done")
                            .primary()
                            .on_click(move |window, cx| {
                                done_this.update(cx, |screen, cx| screen.apply_editor(window, cx));
                            }),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_validates() {
        let mut editor = PositionEditor::from_position(&Chess::default());
        assert_eq!(editor.castling, [true; 4]);
        assert!(editor.position().is_ok());
        // Taking the h1 rook away drops that right instead of refusing the position.
        editor.paint(Square::H1, true);
        let pos = editor.position().unwrap();
        assert!(!pos.castles().castling_rights().contains(Square::H1));
        editor.board = Board::empty();
        assert!(editor.position().unwrap_err().starts_with("Empty board"));
        editor.brush = Some(Piece {
            color: Color::White,
            role: Role::King,
        });
        editor.paint(Square::E1, false);
        assert_eq!(editor.board.piece_at(Square::E1), editor.brush);
        editor.paint(Square::E1, false);
        assert_eq!(editor.board.piece_at(Square::E1), None);
    }
}
