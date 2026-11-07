//! OTB-style scoresheet rendering of a [`GameTree`].

use std::rc::Rc;

use chess_core::{GameTree, NodeId, Notation, display_san, nag_symbol};
use gpui::{
    App, Div, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent, SharedString,
    StatefulInteractiveElement, Window, div, prelude::*, px, rgb,
};

use crate::{
    theme::{MONO, SANS, T},
    widgets::{h_flex, v_flex},
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
                .when(current, |pill| {
                    pill.bg(rgb(T.accent)).text_color(rgb(T.accent_fg))
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
}

fn inline_move(
    tree: &GameTree,
    id: NodeId,
    numbered: bool,
    props: &ScoresheetProps,
) -> impl IntoElement + use<> {
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
    let current = props.current == id;
    let select = props.on_select.clone();
    let context = props.on_context.clone();
    div()
        .id(("var-mv", id))
        .cursor_pointer()
        .px(px(2.))
        .rounded(px(3.))
        .when(current, |pill| {
            pill.bg(rgb(T.accent)).text_color(rgb(T.accent_fg))
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
        .text_size(px(13.))
        .line_height(px(21.))
        .text_color(rgb(T.fg_2));
    let mut current = start;
    let mut numbered = true;
    let mut nested: Vec<Div> = Vec::new();
    loop {
        flow = flow.child(inline_move(tree, current, numbered, props));
        numbered = false;
        let node = tree.node(current);
        if !node.comment.is_empty() {
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
    div()
        .ml(px(47.))
        .mr(px(14.))
        .my(px(4.))
        .font_family(SANS)
        .italic()
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(rgb(T.fg_2))
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
            .border_b_1()
            .border_color(rgb(T.divider))
            .child(
                div()
                    .w(px(NUMBER_W - 4.))
                    .flex_none()
                    .text_color(rgb(T.faint))
                    .child(format!("{number}.")),
            )
    };

    // Root comment and root alternatives.
    let root = tree.node(GameTree::ROOT);
    if !root.comment.is_empty() {
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
                    .w(px(MOVE_W))
                    .flex_none()
                    .child(move_cell(tree, white, props)),
            ),
            None => cells.child(
                div()
                    .w(px(MOVE_W))
                    .flex_none()
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
            if !move_node.comment.is_empty() {
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
                    .w(px(MOVE_W))
                    .flex_none()
                    .pl(px(6.))
                    .text_color(rgb(T.faint))
                    .child("…"),
            );
        }
        body = body.child(cells.child(div().pl(px(6.)).text_color(rgb(T.faint)).child("?")));
    }
    body
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
