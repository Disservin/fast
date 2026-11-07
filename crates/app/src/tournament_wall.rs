//! Every game in progress in one maximized window, for watching a tournament on a second
//! screen. It reads the same event stream as the tournament tab and draws its own boards.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FontWeight, IntoElement, Render, SharedString,
    Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use gpui_component::Root;
use ui::{
    BoardPrefs, BoardView, T,
    board::snap_board_size,
    widgets::{h_flex, v_flex},
};

use crate::{
    model::{AppModel, ModelEvent},
    screens::tournament::game_card,
    tournament_live::{LiveGame, LiveGames},
};

const REFRESH: Duration = Duration::from_millis(200);

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
        cx.notify();
        true
    }
}

impl Render for LiveWall {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.games.len();
        // A square-ish grid, with the boards as large as the window allows.
        let cols = ((count as f32).sqrt().ceil() as usize).max(1);
        let rows = count.div_ceil(cols).max(1);
        let viewport = window.viewport_size();
        // Room for the margins and gaps, and for each card's names, clocks and footer.
        let across =
            (viewport.width - px(48.) - px(20.) * (cols as f32 - 1.)) / cols as f32 - px(40.);
        let down =
            (viewport.height - px(120.) - px(20.) * (rows as f32 - 1.)) / rows as f32 - px(96.);
        let edge = snap_board_size(across.min(down).max(px(160.)), window.scale_factor());

        let title = match self.names.as_slice() {
            [a, b, ..] => format!("{a} vs {b}"),
            _ => "Live games".to_owned(),
        };
        let status = if count == 0 {
            "No games in progress".to_owned()
        } else {
            format!("{count} in progress")
        };
        let grid =
            h_flex()
                .flex_wrap()
                .items_start()
                .gap(px(20.))
                .children(self.games.iter().filter_map(|game| {
                    let (board, _) = self.boards.get(&game.slot)?;
                    Some(game_card(game, board, edge, &self.names, false))
                }));

        v_flex()
            .size_full()
            .overflow_hidden()
            .p(px(24.))
            .gap(px(20.))
            .bg(rgb(T.tour_bg))
            .child(
                h_flex()
                    .gap(px(14.))
                    .items_center()
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
            .child(grid)
    }
}
