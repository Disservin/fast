//! The menus and keyboard shortcuts: which action a key runs, and binding keys to actions.

use super::*;

/// The application menus: native on macOS, drawn in the top bar elsewhere.
pub(super) fn app_menus() -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu::new(ui::i18n::t("File")).items([
            MenuItem::action(ui::i18n::t("New game"), NewGame),
            MenuItem::action(ui::i18n::t("New Chess960 game"), NewChess960),
            MenuItem::action(ui::i18n::t("New tab"), NewTab),
            MenuItem::action(ui::i18n::t("Close tab"), CloseTab),
            MenuItem::action(ui::i18n::t("Reopen closed tab"), ReopenTab),
            MenuItem::action(ui::i18n::t("Open PGN or FEN…"), OpenGame),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Save to My games"), SaveGame),
            MenuItem::action(ui::i18n::t("Save as PGN…"), SavePgnAs),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Settings"), ShowSettings),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Quit"), Quit),
        ]),
        Menu::new(ui::i18n::t("Edit")).items([
            MenuItem::action(ui::i18n::t("Copy FEN"), CopyFen),
            MenuItem::action(ui::i18n::t("Copy PGN"), CopyPgn),
            MenuItem::action("Paste FEN or PGN", PasteGame),
        ]),
        Menu::new(ui::i18n::t("View")).items([
            MenuItem::action(ui::i18n::t("Analyse"), ShowAnalyse),
            MenuItem::action(ui::i18n::t("Database"), ShowDatabase),
            MenuItem::action(ui::i18n::t("Train"), ShowTrain),
            MenuItem::action(ui::i18n::t("Play"), ShowPlay),
            MenuItem::action(ui::i18n::t("Next tab"), NextTab),
            MenuItem::action(ui::i18n::t("Previous tab"), PrevTab),
            MenuItem::action(ui::i18n::t("Go back"), TabBack),
            MenuItem::action(ui::i18n::t("Go forward"), TabForward),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Flip board"), FlipBoard),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Start of game", FirstMove),
            MenuItem::action(ui::i18n::t("Previous move"), PrevMove),
            MenuItem::action(ui::i18n::t("Next move"), NextMove),
            MenuItem::action("End of line", LastMove),
        ]),
        Menu::new(ui::i18n::t("Engine")).items([
            MenuItem::action("Engine on / off", ToggleEngine),
            MenuItem::action("Pause / resume", PauseEngine),
            MenuItem::action("Analyse game backwards", AnalyseBackward),
            MenuItem::separator(),
            MenuItem::action("Engine output window", EngineOutput),
        ]),
    ]
}

/// Whether the menus are drawn by the app rather than the system.
pub(super) fn draws_own_menus() -> bool {
    !cfg!(target_os = "macos")
}

pub fn action_for(name: &str) -> Option<(Box<dyn Action>, &'static str)> {
    Some(match name {
        "prev_move" => (Box::new(PrevMove), SHORTCUT_CONTEXT),
        "next_move" => (Box::new(NextMove), SHORTCUT_CONTEXT),
        "first_move" => (Box::new(FirstMove), SHORTCUT_CONTEXT),
        "last_move" => (Box::new(LastMove), SHORTCUT_CONTEXT),
        "toggle_engine" => (Box::new(ToggleEngine), SPACE_CONTEXT),
        "flip_board" => (Box::new(FlipBoard), SHORTCUT_CONTEXT),
        "search" => (Box::new(FocusSearch), SHORTCUT_CONTEXT),
        "save_game" => (Box::new(SaveGame), SHORTCUT_CONTEXT),
        "new_game" => (Box::new(NewGame), SHORTCUT_CONTEXT),
        "new_chess960" => (Box::new(NewChess960), SHORTCUT_CONTEXT),
        "new_tab" => (Box::new(NewTab), TAB_CONTEXT),
        "close_tab" => (Box::new(CloseTab), TAB_CONTEXT),
        "reopen_tab" => (Box::new(ReopenTab), TAB_CONTEXT),
        "next_tab" => (Box::new(NextTab), TAB_CONTEXT),
        "prev_tab" => (Box::new(PrevTab), TAB_CONTEXT),
        "next_tab_alt" => (Box::new(NextTab), TAB_CONTEXT),
        "prev_tab_alt" => (Box::new(PrevTab), TAB_CONTEXT),
        "tab_back" => (Box::new(TabBack), TAB_CONTEXT),
        "tab_forward" => (Box::new(TabForward), TAB_CONTEXT),
        "open_game" => (Box::new(OpenGame), SHORTCUT_CONTEXT),
        "save_pgn_as" => (Box::new(SavePgnAs), SHORTCUT_CONTEXT),
        "paste_game" => (Box::new(PasteGame), SHORTCUT_CONTEXT),
        "copy_fen" => (Box::new(CopyFen), SHORTCUT_CONTEXT),
        "copy_pgn" => (Box::new(CopyPgn), SHORTCUT_CONTEXT),
        "pause_engine" => (Box::new(PauseEngine), SHORTCUT_CONTEXT),
        "analyse_backward" => (Box::new(AnalyseBackward), SHORTCUT_CONTEXT),
        "more_lines" => (Box::new(MoreLines), SHORTCUT_CONTEXT),
        "fewer_lines" => (Box::new(FewerLines), SHORTCUT_CONTEXT),
        "engine_output" => (Box::new(EngineOutput), SHORTCUT_CONTEXT),
        "all_moves" => (Box::new(ToggleAllMoves), SHORTCUT_CONTEXT),
        "quality_arrows" => (Box::new(ToggleArrows), SHORTCUT_CONTEXT),
        "sort_lines" => (Box::new(ToggleSortLines), SHORTCUT_CONTEXT),
        "next_graph" => (Box::new(NextGraph), SHORTCUT_CONTEXT),
        "scoresheet_style" => (Box::new(ToggleScoresheet), SHORTCUT_CONTEXT),
        "show_analyse" => (Box::new(ShowAnalyse), SHORTCUT_CONTEXT),
        "show_database" => (Box::new(ShowDatabase), SHORTCUT_CONTEXT),
        "show_train" => (Box::new(ShowTrain), SHORTCUT_CONTEXT),
        "show_play" => (Box::new(ShowPlay), SHORTCUT_CONTEXT),
        "show_settings" => (Box::new(ShowSettings), TAB_CONTEXT),
        "next_explorer_db" => (Box::new(NextExplorerDb), SHORTCUT_CONTEXT),
        "quit" => (Box::new(Quit), SHORTCUT_CONTEXT),
        _ => return None,
    })
}

/// Binds the configured keystrokes, skipping any that do not parse.
pub fn bind_shortcuts(shortcuts: &BTreeMap<String, String>, cx: &mut App) {
    for (name, keystroke) in shortcuts {
        // Cleared in Settings.
        if keystroke.is_empty() {
            continue;
        }
        let Some((action, context)) = action_for(name) else {
            continue;
        };
        load_binding(keystroke, action, context, cx);
    }
}

pub(super) fn load_binding(keystroke: &str, action: Box<dyn Action>, context: &str, cx: &mut App) {
    let predicate = gpui::KeyBindingContextPredicate::parse(context)
        .map(Into::into)
        .ok();
    match KeyBinding::load(keystroke, action, predicate, false, None, cx) {
        Ok(binding) => cx.bind_keys([binding]),
        Err(err) => log::warn!("ignoring shortcut {keystroke}: {err}"),
    }
}

/// Removes a shortcut so it can be rebound.
pub fn unbind_shortcut(keystroke: &str, name: &str, cx: &mut App) {
    if keystroke.is_empty() {
        return;
    }
    if let Some((_, context)) = action_for(name) {
        load_binding(keystroke, Box::new(gpui::NoAction), context, cx);
    }
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    refresh_menus(cx);
}

pub fn refresh_menus(cx: &mut App) {
    cx.set_menus(app_menus());
    gpui_component::GlobalState::global_mut(cx)
        .set_app_menus(app_menus().into_iter().map(gpui::Menu::owned).collect());
}
