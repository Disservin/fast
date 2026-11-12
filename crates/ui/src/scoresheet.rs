//! OTB-style scoresheet rendering of a [`GameTree`].

use std::{cell::Cell, collections::HashMap, rc::Rc};

use chess_core::{GameTree, NodeId, Notation, display_san, nag_symbol};
use gpui::{
    App, Bounds, Div, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent, Pixels,
    SharedString, StatefulInteractiveElement, Window, canvas, div, prelude::*, px, rgb,
};

use crate::{
    theme::{MONO, SANS, T},
    widgets::{StyleExt, h_flex, v_flex},
};

const NUMBER_W: f32 = 42.;
const MOVE_W: f32 = 114.;
const ROW_H: f32 = 25.;

pub type SelectHandler = Rc<dyn Fn(NodeId, &mut Window, &mut App)>;
pub type ContextHandler = Rc<dyn Fn(NodeId, &MouseDownEvent, &mut Window, &mut App)>;

pub struct ScoresheetProps {
    pub current: NodeId,
    pub notation: Notation,
    pub on_select: SelectHandler,
    pub on_context: ContextHandler,
    /// The evaluation after each move, already written for display ("+0.35", "#-3"),
    /// shown beside the move.
    pub evals: Rc<HashMap<NodeId, SharedString>>,
    /// Where the current move was last drawn, so the sheet can be scrolled to keep it in view.
    pub current_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

/// An invisible layer over the current move that records where it is drawn.
fn bounds_probe(slot: &Rc<Cell<Option<Bounds<Pixels>>>>) -> impl IntoElement + use<> {
    let slot = slot.clone();
    canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

fn nag_text(nags: &[u8]) -> Option<String> {
    let mut out = String::new();
    for &nag in nags {
        if let Some(symbol) = nag_symbol(nag) {
            // Position judgements stand apart from the move: "15.Bd3 =".
            if nag >= 10 {
                out.push(' ');
            }
            out.push_str(symbol);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn move_cell(tree: &GameTree, id: NodeId, props: &ScoresheetProps) -> impl IntoElement + use<> {
    let node = tree.node(id);
    let text = display_san(&node.san, node.mv.as_ref(), props.notation);
    let current = props.current == id;
    let select = props.on_select.clone();
    let context = props.on_context.clone();
    let hover: Hsla = rgb(T.hover).into();
    h_flex()
        .id(("mv", id))
        .cursor_pointer()
        .on_click(move |_, window, cx| select(id, window, cx))
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            context(id, event, window, cx)
        })
        .child(
            div()
                .px(px(6.))
                .h(px(21.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .relative()
                .when(current, |pill| {
                    pill.bg(rgb(T.accent))
                        .text_color(rgb(T.accent_fg))
                        .child(bounds_probe(&props.current_bounds))
                })
                .when(!current, |pill| pill.hover(move |style| style.bg(hover)))
                .child(text),
        )
        .children(nag_text(&node.nags).map(|nags| {
            div()
                .text_color(rgb(T.warn))
                .font_weight(FontWeight::SEMIBOLD)
                .child(nags)
        }))
        .children(eval_text(id, props))
}

/// The evaluation after a move, small and `muted` so the moves stay the thing to read.
fn eval_text(id: NodeId, props: &ScoresheetProps) -> Option<impl IntoElement + use<>> {
    let text = props.evals.get(&id)?.clone();
    Some(
        div()
            .ml(px(4.))
            .flex_none()
            .whitespace_nowrap()
            .text_as(11., T.muted)
            .child(text),
    )
}

/// A move as written in the variation text: numbered when it starts a line or is
/// White's, followed by its annotation symbols.
fn move_text(tree: &GameTree, id: NodeId, numbered: bool, props: &ScoresheetProps) -> String {
    let node = tree.node(id);
    let san = display_san(&node.san, node.mv.as_ref(), props.notation);
    let mut text = if numbered || node.ply % 2 == 1 {
        chess_core::notation::numbered(node.ply, &san)
    } else {
        san
    };
    if let Some(nags) = nag_text(&node.nags) {
        text.push_str(&nags);
    }
    text
}

fn inline_move(
    tree: &GameTree,
    id: NodeId,
    numbered: bool,
    props: &ScoresheetProps,
) -> impl IntoElement + use<> {
    let text = move_text(tree, id, numbered, props);
    let current = props.current == id;
    let select = props.on_select.clone();
    let context = props.on_context.clone();
    div()
        .id(("var-mv", id))
        .cursor_pointer()
        .px(px(2.))
        .rounded(px(3.))
        .relative()
        .when(current, |pill| {
            pill.bg(rgb(T.accent))
                .text_color(rgb(T.accent_fg))
                .child(bounds_probe(&props.current_bounds))
        })
        .when(!current, |pill| pill.hover(|style| style.bg(rgb(T.hover))))
        .on_click(move |_, window, cx| select(id, window, cx))
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            context(id, event, window, cx)
        })
        .child(text)
}

/// Variation blocks: one indented block per alternative, recursing into sub-variations.
fn variation_block(tree: &GameTree, start: NodeId, props: &ScoresheetProps, depth: u32) -> Div {
    let mut flow = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x(px(4.))
        .font_family(MONO)
        .text_as(13., T.fg_2)
        .line_height(px(21.));
    let mut current = start;
    let mut numbered = true;
    let mut nested: Vec<Div> = Vec::new();
    loop {
        flow = flow.child(inline_move(tree, current, numbered, props));
        numbered = false;
        let node = tree.node(current);
        if has_text(&node.comment) {
            nested.push(comment(&node.comment));
        }
        let Some((&next, others)) = node.children.split_first() else {
            break;
        };
        // Alternatives to `next` hang under the block.
        for &other in others {
            nested.push(variation_block(tree, other, props, depth + 1));
        }
        current = next;
    }
    v_flex()
        .ml(px(if depth == 0 { 48. } else { 0. }))
        .mr(px(14.))
        .my(px(4.))
        .pl(px(10.))
        .border_l_2()
        .border_color(rgb(T.border_strong))
        .child(flow)
        .children(nested)
}

fn comment(text: &str) -> Div {
    // Clock and eval commands are data for the graphs, not prose.
    let text = chess_core::strip_commands(text);
    div()
        .ml(px(47.))
        .mr(px(14.))
        .my(px(4.))
        .font_family(SANS)
        .italic()
        .text_as(13., T.fg_2)
        .line_height(px(20.))
        .child(SharedString::from(text.to_owned()))
}

/// Builds the scoresheet body: one row per full move, with comments and variations
/// inserted under the move they belong to, and a "next move" row at the end.
pub fn scoresheet_body(tree: &GameTree, props: &ScoresheetProps) -> impl IntoElement + use<> {
    let mut body = v_flex().w_full().font_family(MONO).text_size(px(14.));
    let main = tree.mainline();
    let mut index = 0;

    let row = |number: u32| {
        h_flex()
            .w_full()
            .h(px(ROW_H))
            .flex_none()
            .pl(px(18.))
            .border_b_as(T.divider)
            .child(
                div()
                    .col_w(NUMBER_W - 4.)
                    .text_color(rgb(T.faint))
                    .child(format!("{number}.")),
            )
    };

    // Root comment and root alternatives.
    let root = tree.node(GameTree::ROOT);
    if has_text(&root.comment) {
        body = body.child(comment(&root.comment));
    }

    while index < main.len() {
        let id = main[index];
        let node = tree.node(id);
        let number = node.ply.div_ceil(2);
        let mut cells = row(number).text_color(rgb(T.fg));
        let white_id = (node.ply % 2 == 1).then_some(id);
        let black_id = if white_id.is_some() {
            main.get(index + 1).copied()
        } else {
            Some(id)
        };
        cells = match white_id {
            Some(white) => cells.child(
                div()
                    .col_w(MOVE_W)
                    .overflow_hidden()
                    .child(move_cell(tree, white, props)),
            ),
            None => cells.child(
                div()
                    .col_w(MOVE_W)
                    .pl(px(6.))
                    .text_color(rgb(T.faint))
                    .child("…"),
            ),
        };
        if let Some(black) = black_id {
            cells = cells.child(move_cell(tree, black, props));
        } else if !node.pos.is_game_over_like() {
            // Black to reply: the prompt shares White's row.
            cells = cells.child(div().pl(px(6.)).text_color(rgb(T.faint)).child("?"));
        }
        body = body.child(cells);
        index += 1;
        if white_id.is_some() && black_id.is_some() {
            index += 1;
        }

        // Annotations of the moves just shown, white's first.
        let shown: Vec<NodeId> = white_id.into_iter().chain(black_id).collect();
        for &move_id in &shown {
            let move_node = tree.node(move_id);
            if has_text(&move_node.comment) {
                body = body.child(comment(&move_node.comment));
            }
            // Alternatives to this move hang off its parent.
            if let Some(parent) = move_node.parent {
                let siblings = &tree.node(parent).children;
                if siblings.first() == Some(&move_id) {
                    for &variation in &siblings[1..] {
                        body = body.child(variation_block(tree, variation, props, 0));
                    }
                }
            }
        }
    }

    // White to move next gets a fresh row with the prompt. After a Black start position
    // with no moves yet, the prompt follows an ellipsis instead.
    let end = main.last().copied().unwrap_or(GameTree::ROOT);
    let end_node = tree.node(end);
    let next_ply = end_node.ply + 1;
    if !end_node.pos.is_game_over_like() && (next_ply % 2 == 1 || main.is_empty()) {
        let mut cells = row(next_ply.div_ceil(2)).border_b_0();
        if next_ply.is_multiple_of(2) {
            cells = cells.child(
                div()
                    .col_w(MOVE_W)
                    .pl(px(6.))
                    .text_color(rgb(T.faint))
                    .child("…"),
            );
        }
        body = body.child(cells.child(div().pl(px(6.)).text_color(rgb(T.faint)).child("?")));
    }
    body
}

/// A comment with something to read once `[%clk]`-style commands are removed.
fn has_text(comment: &str) -> bool {
    !comment.is_empty() && !chess_core::strip_commands(comment).is_empty()
}

/// Colours for variation depths in the inline view: main line, then each nesting level.
pub fn depth_color(depth: usize) -> u32 {
    let colors = [T.fg, T.accent, T.good, T.warn, T.q_blunder];
    if depth == 0 {
        colors[0]
    } else {
        colors[1 + (depth - 1) % 4]
    }
}

/// One piece of running text in the inline view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Move {
        id: NodeId,
        numbered: bool,
        depth: usize,
    },
    Open(usize),
    Close(usize),
    Comment(String),
}

/// The game as running text, PGN-like: a move's alternatives follow it in
/// parentheses, so `1. e4 (1. d4 d5) 1... e5`.
pub fn inline_tokens(tree: &GameTree) -> Vec<Token> {
    fn line(tree: &GameTree, start: NodeId, depth: usize, out: &mut Vec<Token>) {
        let mut current = start;
        let mut numbered = true;
        loop {
            out.push(Token::Move {
                id: current,
                numbered,
                depth,
            });
            numbered = false;
            let node = tree.node(current);
            if has_text(&node.comment) {
                out.push(Token::Comment(chess_core::strip_commands(&node.comment)));
                numbered = true;
            }
            // Alternatives to this move hang off its parent and follow it.
            if let Some(parent) = node.parent {
                let siblings = &tree.node(parent).children;
                if siblings.first() == Some(&current) {
                    for &other in &siblings[1..] {
                        out.push(Token::Open(depth + 1));
                        line(tree, other, depth + 1, out);
                        out.push(Token::Close(depth + 1));
                        numbered = true;
                    }
                }
            }
            match node.children.first() {
                Some(&next) => current = next,
                None => break,
            }
        }
    }
    let mut out = Vec::new();
    let root = tree.node(GameTree::ROOT);
    if has_text(&root.comment) {
        out.push(Token::Comment(chess_core::strip_commands(&root.comment)));
    }
    if let Some(&first) = root.children.first() {
        line(tree, first, 0, &mut out);
    }
    out
}

/// Nibbler-style scoresheet: running text, variations coloured by depth.
pub fn scoresheet_inline(tree: &GameTree, props: &ScoresheetProps) -> impl IntoElement + use<> {
    let mut flow = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline()
        .gap_x(px(4.))
        .gap_y(px(2.))
        .px(px(16.))
        .py(px(6.))
        .font_family(MONO)
        .text_size(px(14.))
        .line_height(px(22.));
    for token in inline_tokens(tree) {
        flow = match token {
            Token::Move {
                id,
                numbered,
                depth,
            } => {
                let text = move_text(tree, id, numbered, props);
                let current = props.current == id;
                let select = props.on_select.clone();
                let context = props.on_context.clone();
                flow.child(
                    div()
                        .id(("inline-mv", id))
                        .cursor_pointer()
                        .px(px(3.))
                        .rounded(px(3.))
                        .text_color(rgb(depth_color(depth)))
                        .when(depth == 0, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .when(depth > 0, |el| el.text_size(px(13.)))
                        .relative()
                        .when(current, |pill| {
                            pill.bg(rgb(T.accent))
                                .text_color(rgb(T.accent_fg))
                                .child(bounds_probe(&props.current_bounds))
                        })
                        .when(!current, |pill| pill.hover(|style| style.bg(rgb(T.hover))))
                        .on_click(move |_, window, cx| select(id, window, cx))
                        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            context(id, event, window, cx)
                        })
                        .child(text),
                )
                .when(depth == 0, |flow| flow.children(eval_text(id, props)))
            }
            Token::Open(depth) => flow.child(div().text_color(rgb(depth_color(depth))).child("(")),
            Token::Close(depth) => flow.child(div().text_color(rgb(depth_color(depth))).child(")")),
            Token::Comment(text) => flow.child(
                div()
                    .font_family(SANS)
                    .italic()
                    .text_as(13., T.muted)
                    .child(SharedString::from(text)),
            ),
        };
    }
    flow
}

trait GameOverLike {
    fn is_game_over_like(&self) -> bool;
}

impl GameOverLike for chess_core::Chess {
    fn is_game_over_like(&self) -> bool {
        use chess_core::Position as _;
        self.is_game_over()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tokens_put_alternatives_after_their_move() {
        let tree = chess_core::parse_pgn("1. e4 {best} (1. d4 d5 (1... Nf6)) 1... e5 *")
            .unwrap()
            .remove(0);
        let tokens = inline_tokens(&tree);
        let shape: Vec<String> = tokens
            .iter()
            .map(|token| match token {
                Token::Move { id, depth, .. } => format!("{}@{depth}", tree.node(*id).san),
                Token::Open(depth) => format!("({depth}"),
                Token::Close(depth) => format!("){depth}"),
                Token::Comment(text) => format!("{{{text}}}"),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                "e4@0", "{best}", "(1", "d4@1", "d5@1", "(2", "Nf6@2", ")2", ")1", "e5@0"
            ]
        );
        // After a comment or a variation the next move repeats its number.
        let numbered: Vec<bool> = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Move { numbered, .. } => Some(*numbered),
                _ => None,
            })
            .collect();
        assert_eq!(numbered, vec![true, true, false, true, true]);
    }

    #[test]
    fn clock_only_comments_are_hidden() {
        let tree = chess_core::parse_pgn("1. e4 {[%clk 0:01:00]} e5 *")
            .unwrap()
            .remove(0);
        assert!(
            !inline_tokens(&tree)
                .iter()
                .any(|token| matches!(token, Token::Comment(_)))
        );
        assert_ne!(depth_color(1), depth_color(2));
        assert_eq!(depth_color(1), depth_color(5));
    }
}
