//! Live games as cards: the board, both players, their searches and the evaluation graph. The watch-all window uses them too.

use super::*;

/// Name, score and clock beside a live board: Black above it, White below. Scores are
/// from White's side.
pub(super) fn player_line(game: &LiveGame, white: bool, size: f32) -> impl IntoElement + use<> {
    let (name, clock, search) = if white {
        (&game.white, game.wtime, &game.white_search)
    } else {
        (&game.black, game.btime, &game.black_search)
    };
    let thinking = game.thinking.is_some() && game.thinking == *name;
    h_flex()
        .gap(px(6.))
        .text_size(px(size))
        .child(
            div()
                .size(px(7.))
                .flex_none()
                .rounded_full()
                .when(thinking, |dot| dot.bg(rgb(T.good))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(
                    name.clone().unwrap_or_else(|| "…".into()),
                )),
        )
        .child(
            div()
                .font_family(MONO)
                .text_color(rgb(T.muted))
                .child(SharedString::from(search_text(search))),
        )
        .child(div().font_family(MONO).child(SharedString::from(
            clock.map(tournament_live::format_clock).unwrap_or_default(),
        )))
}

/// Engine A is blue, engine B orange, any others grey.
pub(super) fn engine_colour(names: &[String], name: &str) -> u32 {
    match names.iter().position(|engine| engine == name) {
        Some(0) => T.engine_a,
        Some(1) => T.engine_b,
        _ => T.muted,
    }
}

/// Where the eval bar's white part ends, from 0 (Black ahead) to 1 (White ahead).
pub(super) fn white_share(score: Option<Score>) -> f32 {
    match score {
        None => 0.5,
        Some(Score::Mate(moves)) => {
            if moves > 0 {
                1.
            } else {
                0.
            }
        }
        Some(Score::Cp(cp)) => 1. / (1. + 10f32.powf(-(cp as f32) / 400.)),
    }
}

/// The search whose line a game card shows: whoever is thinking, else the last one with a score.
pub(super) fn shown_search(game: &LiveGame) -> &Search {
    let black_turn = game.thinking.is_some() && game.thinking == game.black;
    if black_turn {
        &game.black_search
    } else if game.white_search.score.is_some() || game.thinking.is_some() {
        &game.white_search
    } else {
        &game.black_search
    }
}

/// The evaluation as a vertical bar: White's share fills from the bottom.
pub(super) fn eval_bar(game: &LiveGame, edge: Pixels) -> ui::eval_bar::EvalBar {
    let share = white_share(shown_search(game).score);
    ui::eval_bar::EvalBar::new(("game-eval", game.number as usize), share, edge)
}

/// A player's name, colour and clock; the side to move gets a "to move" label and a lit clock.
pub(super) fn player_row(game: &LiveGame, white: bool, names: &[String], to_move: bool) -> Div {
    let (name, clock) = if white {
        (&game.white, game.wtime)
    } else {
        (&game.black, game.btime)
    };
    let label = name.clone().unwrap_or_else(|| "…".into());
    let clock = clock
        .map(tournament_live::format_clock)
        .unwrap_or_else(|| "–".into());
    h_flex()
        .h(px(26.))
        .flex_none()
        .gap(px(8.))
        .items_center()
        .text_size(px(13.))
        .child(swatch(engine_colour(names, &label)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(label)),
        )
        .children(to_move.then(|| div().text_as(11., T.live_fg).child(ui::i18n::t("to move"))))
        .child(
            div()
                .px(px(8.))
                .py(px(2.))
                .rounded(px(6.))
                .font_family(MONO)
                .bg(rgb(if to_move { T.live_bg } else { T.tour_tile }))
                .text_color(rgb(if to_move { T.live_fg } else { T.fg_2 }))
                .child(SharedString::from(clock)),
        )
}

/// One game: Black, the board with its eval bar, White, then the game number and evaluation.
/// Used by the tab and by the watch-all window.
pub(crate) fn game_card(
    game: &LiveGame,
    board: &Entity<BoardView>,
    edge: Pixels,
    names: &[String],
    focused: bool,
) -> Div {
    let (pos, _) = game.position();
    let black_to_move = pos.turn().is_black();
    let search = shown_search(game);
    let eval = search
        .score
        .map(Score::format)
        .unwrap_or_else(|| "–".into());
    let depth = search
        .depth
        .map(|depth| format!(" / d{depth}"))
        .unwrap_or_default();
    v_flex()
        .w(edge + px(40.))
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(12.))
        .bg(rgb(T.tour_card))
        .border_1()
        .border_color(rgb(if focused {
            T.border_strong
        } else {
            T.tour_card_border
        }))
        .child(player_row(game, false, names, black_to_move))
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(eval_bar(game, edge))
                .child(div().size(edge).child(board.clone())),
        )
        .child(player_row(game, true, names, !black_to_move))
        .child(
            h_flex()
                .h(px(18.))
                .flex_none()
                .items_center()
                .justify_between()
                .text_as(12., T.muted)
                .child(SharedString::from(format!("Game #{}", game.number)))
                .child(SharedString::from(format!("{eval}{depth}"))),
        )
}

/// The game's eval after each move, one line per engine in its tournament colour.
/// Shown only for the game being watched on its own.
pub(crate) fn eval_graph(game: &LiveGame, names: &[String], height: f32) -> Div {
    let cp = |score: Score| match score {
        Score::Cp(cp) => cp as f32,
        Score::Mate(moves) => {
            chess_core::annotation::MATE_CP as f32 * if moves > 0 { 1. } else { -1. }
        }
    };
    // Ply 1 is White's move, so even indices are White's engine and odd ones Black's.
    let side = |white: bool| {
        let name = if white { &game.white } else { &game.black };
        let colour = name
            .as_deref()
            .map_or(T.muted, |name| engine_colour(names, name));
        let values = game
            .evals
            .iter()
            .enumerate()
            .map(|(index, score)| score.filter(|_| (index % 2 == 0) == white).map(cp))
            .collect();
        (name.clone().unwrap_or_else(|| "?".into()), colour, values)
    };
    let sides = [side(true), side(false)];
    let legend = h_flex()
        .gap(px(14.))
        .text_as(12., T.fg_2)
        .children(sides.iter().map(|(name, colour, _)| {
            h_flex()
                .gap(px(6.))
                .items_center()
                .min_w_0()
                .child(swatch(*colour))
                .child(SharedString::from(name.clone()))
        }));
    let count = game.moves.len();
    let graph = ui::graph::graph(ui::graph::GraphProps {
        series: sides
            .into_iter()
            .map(|(_, color, values)| ui::graph::Series {
                values,
                color,
                fill: false,
                connect: true,
            })
            .collect(),
        count,
        current: count.checked_sub(1),
        scale: ui::graph::Scale::SymLog {
            max: chess_core::annotation::MATE_CP as f32,
            unit: 100.0,
        },
        // ±1 would crowd 0 and ±3 at these heights.
        ticks: ui::graph::eval_ticks()
            .into_iter()
            .filter(|(cp, _)| cp.abs() != 100.0)
            .collect(),
        height: Some(height),
        on_scrub: std::rc::Rc::new(|_, _, _| {}),
        empty: "No evaluations yet. Each engine's score appears after its move.",
    });
    v_flex()
        .gap(px(6.))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(10.))
                .child(section_label("Eval"))
                .child(legend),
        )
        .child(graph)
}

/// Result chip colours: draws grey, engine A's wins blue, engine B's wins orange.
pub(super) fn chip_colours(row: &GameRow, names: &[String]) -> (u32, u32) {
    let winner = match row.result.as_str() {
        "1-0" => Some(&row.white),
        "0-1" => Some(&row.black),
        _ => None,
    };
    match winner {
        None => (T.chip_draw_bg, T.chip_draw_fg),
        Some(name) if names.first() == Some(name) => (T.chip_a_bg, T.chip_a_fg),
        Some(name) if names.get(1) == Some(name) => (T.chip_b_bg, T.chip_b_fg),
        Some(_) => (T.tour_tile, T.fg_2),
    }
}

pub(super) fn name_cell(name: &str, names: &[String]) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(swatch(engine_colour(names, name)))
                .child(SharedString::from(name.to_owned())),
        )
}

pub(super) fn search_text(search: &Search) -> String {
    let Some(score) = search.score else {
        return String::new();
    };
    let depth = search
        .depth
        .map(|depth| format!("/{depth}"))
        .unwrap_or_default();
    format!("{}{depth}", score.format())
}

/// An engine's search as columns: Eval, Depth and Nodes over their values.
pub(crate) fn search_stats(search: &Search) -> Div {
    let dash = || "–".to_owned();
    let column = |label: &'static str, value: String| {
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(div().text_as(11., T.muted).child(ui::i18n::t(label)))
            .child(
                div()
                    .font_family(MONO)
                    .text_as(14., T.fg)
                    .whitespace_nowrap()
                    .child(SharedString::from(value)),
            )
    };
    h_flex()
        .gap(px(12.))
        .child(column(
            "Eval",
            search.score.map_or_else(dash, Score::format),
        ))
        .child(column(
            "Depth",
            search.depth.map_or_else(dash, |depth| depth.to_string()),
        ))
        .child(column(
            "Nodes",
            search.nodes.map_or_else(dash, engine::format_nodes),
        ))
}
