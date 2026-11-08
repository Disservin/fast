//! Every game in progress in one maximized window, for watching a tournament on a second
//! screen. It reads the same event stream as the tournament tab and draws its own boards.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use chess_core::{Position as _, format_pv, pv_to_san};
use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, FontWeight, IntoElement,
    KeyDownEvent, Pixels, Render, SharedString, Size, Subscription, Task, TitlebarOptions,
    WeakEntity, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::Root;
use ui::{
    BoardPrefs, BoardView, MONO, T,
    board::snap_board_size,
    widgets::{btn, h_flex, section_label, v_flex},
};

use crate::{
    model::{AppModel, ModelEvent},
    screens::tournament::{game_card, search_text},
    tournament_live::{LiveGame, LiveGames, Search},
};

const REFRESH: Duration = Duration::from_millis(200);
const GAP: Pixels = px(20.);
/// Window padding on each side.
const PAD: Pixels = px(24.);
/// The title row above the grid.
const HEADER_H: Pixels = px(32.);
/// What a game card adds to its board: across, the padding and eval bar; down, the padding,
/// the two player rows and the footer.
const CARD_EXTRA_W: Pixels = px(40.);
const CARD_EXTRA_H: Pixels = px(96.);
const MIN_EDGE: Pixels = px(96.);
/// The side column of the single-game view.
const DETAIL_W: Pixels = px(420.);

/// Picks the column count that gives the largest boards for `count` cards in `viewport`,
/// and returns it with that board edge.
fn layout(count: usize, viewport: Size<Pixels>, scale: f32) -> (usize, Pixels) {
    let width = viewport.width - PAD * 2.;
    let height = viewport.height - PAD * 2. - HEADER_H - GAP;
    let mut best = (1, px(0.));
    for cols in 1..=count.max(1) {
        let rows = count.div_ceil(cols).max(1);
        let across = (width - GAP * (cols as f32 - 1.)) / cols as f32 - CARD_EXTRA_W;
        let down = (height - GAP * (rows as f32 - 1.)) / rows as f32 - CARD_EXTRA_H;
        let edge = across.min(down);
        // Ties go to fewer columns, which leaves fewer holes in the last row.
        if edge > best.1 {
            best = (cols, edge);
        }
    }
    (best.0, snap_board_size(best.1.max(MIN_EDGE), scale))
}

/// Draws `game` on `board`, moving only when the game moved. `shown` is the game and move
/// count the board currently shows.
pub(crate) fn sync_board<C: gpui::AppContext>(
    board: &Entity<BoardView>,
    shown: &mut (u64, usize),
    game: &LiveGame,
    prefs: BoardPrefs,
    cx: &mut C,
) {
    if *shown == (game.number, game.moves.len()) {
        return;
    }
    let animate = shown.0 == game.number && shown.1 + 1 == game.moves.len();
    let (pos, last) = game.position();
    board.update(cx, |board, cx| {
        board.set_prefs(prefs, cx);
        board.set_position(pos, last, animate, cx);
    });
    *shown = (game.number, game.moves.len());
}

pub struct LiveWall {
    model: Entity<AppModel>,
    live: Arc<Mutex<LiveGames>>,
    /// Engine names in the order fastchess was given them; the first two are A and B.
    names: Vec<String>,
    games: Vec<LiveGame>,
    /// The store's change counter when the games were last copied.
    seen: Option<u64>,
    boards: HashMap<usize, (Entity<BoardView>, (u64, usize))>,
    /// The game shown on its own, by game number; the grid when none.
    focused: Option<u64>,
    focus: FocusHandle,
    _poll: Task<()>,
    _prefs: Subscription,
}

impl LiveWall {
    /// Opens the window maximized. It follows the games until it is closed.
    pub fn open(
        model: Entity<AppModel>,
        live: Arc<Mutex<LiveGames>>,
        names: Vec<String>,
        cx: &mut App,
    ) {
        let title: SharedString = match names.as_slice() {
            [a, b, ..] => format!("{a} vs {b} · live games"),
            _ => "Live games".to_owned(),
        }
        .into();
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Maximized(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| Self::new(model, live, names, cx));
            view.read(cx).focus.clone().focus(window, cx);
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            log::error!("could not open the live games window: {err:#}");
        }
    }

    fn new(
        model: Entity<AppModel>,
        live: Arc<Mutex<LiveGames>>,
        names: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let poll = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                match this.update(cx, |wall, cx| wall.refresh(cx)) {
                    Ok(true) => {}
                    _ => return,
                }
            }
        });
        // Recolour the boards on a theme or board change, not only when they next move.
        let prefs = cx.subscribe(&model, |wall, model, event, cx| {
            if matches!(event, ModelEvent::BoardPrefs) {
                let prefs = model.read(cx).config.board.prefs();
                for (board, _) in wall.boards.values() {
                    board.update(cx, |board, cx| board.set_prefs(prefs, cx));
                }
                cx.notify();
            }
        });
        Self {
            model,
            live,
            names,
            games: Vec::new(),
            seen: None,
            boards: HashMap::new(),
            focused: None,
            focus: cx.focus_handle(),
            _poll: poll,
            _prefs: prefs,
        }
    }

    /// Copies the games and moves the boards. Returns false once the window is gone.
    fn refresh(&mut self, cx: &mut Context<Self>) -> bool {
        let games = {
            let Ok(live) = self.live.lock() else {
                return false;
            };
            if self.seen == Some(live.serial) {
                return true;
            }
            self.seen = Some(live.serial);
            live.snapshot()
        };
        let prefs = self.model.read(cx).config.board.prefs();
        for game in &games {
            let entry = self.boards.entry(game.slot).or_insert_with(|| {
                let board = cx.new(|_| BoardView::new(("wall-board", game.slot)).read_only());
                (board, (0, usize::MAX))
            });
            sync_board(&entry.0, &mut entry.1, game, prefs, cx);
        }
        self.games = games;
        // Back to the grid once the game shown on its own has finished.
        if let Some(number) = self.focused
            && !self.games.iter().any(|game| game.number == number)
        {
            self.focused = None;
        }
        cx.notify();
        true
    }

    /// Escape returns to the grid; the arrow keys step through the games in the single-game view.
    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(number) = self.focused else {
            return;
        };
        let step: isize = match event.keystroke.key.as_str() {
            "escape" => {
                self.focused = None;
                cx.notify();
                cx.stop_propagation();
                return;
            }
            "left" | "up" => -1,
            "right" | "down" => 1,
            _ => return,
        };
        let count = self.games.len() as isize;
        if let Some(index) = self.games.iter().position(|game| game.number == number) {
            let next = (index as isize + step).rem_euclid(count) as usize;
            self.focused = Some(self.games[next].number);
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn render_grid(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (cols, edge) = layout(
            self.games.len(),
            window.viewport_size(),
            window.scale_factor(),
        );
        let card = edge + CARD_EXTRA_W;
        let this = cx.entity().downgrade();
        let grid = div()
            .grid()
            .grid_cols(cols as u16)
            .gap(GAP)
            .w(card * cols as f32 + GAP * (cols as f32 - 1.))
            .children(self.games.iter().filter_map(|game| {
                let (board, _) = self.boards.get(&game.slot)?;
                let this = this.clone();
                let number = game.number;
                Some(
                    div()
                        .id(("wall-card", game.slot))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            this.update(cx, |wall, cx| {
                                wall.focused = Some(number);
                                cx.notify();
                            })
                            .ok();
                        })
                        .child(game_card(game, board, edge, &self.names, false)),
                )
            }));
        h_flex()
            .flex_1()
            .min_h_0()
            .justify_center()
            .items_center()
            .child(grid)
            .into_any_element()
    }

    /// One game large, with both engines' searches, the line being searched and the moves.
    fn render_game(&self, game: &LiveGame, window: &Window) -> gpui::AnyElement {
        let Some((board, _)) = self.boards.get(&game.slot) else {
            return div().into_any_element();
        };
        let viewport = window.viewport_size();
        let across = viewport.width - PAD * 2. - GAP - DETAIL_W - CARD_EXTRA_W;
        let down = viewport.height - PAD * 2. - HEADER_H - GAP - CARD_EXTRA_H;
        let edge = snap_board_size(across.min(down).max(MIN_EDGE), window.scale_factor());

        let start = LiveGame {
            moves: Vec::new(),
            ..game.clone()
        }
        .position()
        .0;
        let (pos, _) = game.position();
        let ply = |pos: &chess_core::Chess| {
            (pos.fullmoves().get() - 1) * 2 + u32::from(pos.turn().is_black())
        };
        let moves = format_pv(ply(&start), &pv_to_san(&start, &game.moves));
        let thinking = [
            (&game.white, &game.white_search),
            (&game.black, &game.black_search),
        ]
        .into_iter()
        .find(|(name, _)| game.thinking.is_some() && game.thinking == **name)
        .map(|(_, search)| search);
        let line = thinking
            .map(|search| pv_to_san(&pos, &search.pv))
            .filter(|sans| !sans.is_empty())
            .map(|sans| format_pv(ply(&pos), &sans))
            .unwrap_or_else(|| "–".into());

        let engine = |side: &str, name: &Option<String>, search: &Search| {
            let searching = game.thinking.is_some() && game.thinking == *name;
            v_flex()
                .gap(px(4.))
                .child(section_label(if searching {
                    format!("{side} · thinking")
                } else {
                    side.to_owned()
                }))
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(T.fg))
                        .child(SharedString::from(
                            name.clone().unwrap_or_else(|| "…".into()),
                        )),
                )
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(px(13.))
                        .text_color(rgb(T.fg_2))
                        .child(SharedString::from(match search_text(search, true) {
                            text if text.is_empty() => "–".to_owned(),
                            text => text,
                        })),
                )
        };
        let text_block = |label: &str, text: String| {
            v_flex().gap(px(6.)).child(section_label(label)).child(
                div()
                    .font_family(MONO)
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(rgb(T.fg))
                    .child(SharedString::from(text)),
            )
        };
        let side = v_flex()
            .w(DETAIL_W)
            .max_h(edge + CARD_EXTRA_H)
            .p(px(16.))
            .gap(px(18.))
            .rounded(px(12.))
            .bg(rgb(T.tour_card))
            .border_1()
            .border_color(rgb(T.tour_card_border))
            .child(engine("White", &game.white, &game.white_search))
            .child(engine("Black", &game.black, &game.black_search))
            .child(text_block("Line", line))
            .child(
                div()
                    .id("wall-moves")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(text_block(
                        &format!("Moves · {} plies", game.moves.len()),
                        if moves.is_empty() {
                            "–".into()
                        } else {
                            moves
                        },
                    )),
            );
        h_flex()
            .flex_1()
            .min_h_0()
            .justify_center()
            .items_start()
            .gap(GAP)
            .child(game_card(game, board, edge, &self.names, true))
            .child(side)
            .into_any_element()
    }
}

impl Render for LiveWall {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.games.len();
        let focused = self
            .focused
            .and_then(|number| self.games.iter().find(|game| game.number == number));
        let title = match self.names.as_slice() {
            [a, b, ..] => format!("{a} vs {b}"),
            _ => "Live games".to_owned(),
        };
        let status = match focused {
            Some(game) => format!(
                "Game #{} · Esc or Back for all games · ← → for the others",
                game.number
            ),
            None if count == 0 => "No games in progress".to_owned(),
            None => format!("{count} in progress · click a game to watch it"),
        };
        let back = focused.map(|_| {
            let this = cx.entity().downgrade();
            btn("wall-back", "← All games")
                .compact()
                .on_click(move |_, cx| {
                    this.update(cx, |wall, cx| {
                        wall.focused = None;
                        cx.notify();
                    })
                    .ok();
                })
        });
        let body = match focused {
            Some(game) => self.render_game(game, window),
            None => self.render_grid(window, cx),
        };

        v_flex()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|wall, event, _, cx| wall.on_key(event, cx)))
            .size_full()
            .overflow_hidden()
            .p(PAD)
            .gap(GAP)
            .bg(rgb(T.tour_bg))
            .child(
                h_flex()
                    .h(HEADER_H)
                    .gap(px(14.))
                    .items_center()
                    .children(back)
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(T.fg))
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(status)),
                    ),
            )
            .child(body)
    }
}
