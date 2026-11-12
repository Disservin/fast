//! Workspace tabs: opening, showing, closing, reopening, cycling and renaming them.

use super::*;

impl Workspace {
    pub(super) fn existing_content(&self, mode: Mode) -> Option<TabContent> {
        Some(match mode {
            Mode::Analyse => TabContent::Analyse(self.analyse.clone()),
            Mode::Database => TabContent::Database(self.database.clone()),
            Mode::Train => TabContent::Train(self.train.clone()),
            Mode::Play => TabContent::Play(self.play.clone()),
            Mode::Settings => TabContent::Settings(self.settings.clone()),
            Mode::Tournament => TabContent::Tournament(self.tournament.clone()?),
            Mode::TestPositions => TabContent::TestPositions(self.test_positions.clone()?),
        })
    }

    /// Shows a tab and remembers the one left, so Back returns to it.
    pub(super) fn activate_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        self.show_tab(id, true, cx);
    }

    /// Goes to the tab visited before the current one (`back`) or the one left with Back.
    /// Tabs closed since are skipped.
    pub(super) fn step_history(&mut self, back: bool, cx: &mut Context<Self>) {
        let (from, to) = if back {
            (&mut self.back_tabs, &mut self.forward_tabs)
        } else {
            (&mut self.forward_tabs, &mut self.back_tabs)
        };
        while let Some(id) = from.pop() {
            if id == self.active_tab || !self.tabs.iter().any(|tab| tab.id == id) {
                continue;
            }
            to.push(self.active_tab);
            self.show_tab(id, false, cx);
            return;
        }
    }

    /// Shows a tab. With `record`, the tab left goes into the Back history and the
    /// Forward history is dropped, as in a browser.
    pub(super) fn show_tab(&mut self, id: u64, record: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id).cloned() else {
            return;
        };
        // The new-tab picker covers the content, so going to any tab, even the one
        // already active, dismisses it.
        self.new_tab_menu_open = false;
        if self.active_tab == id {
            cx.notify();
            return;
        }
        if self.mode == Mode::Analyse
            && let Err(err) = self.analyse.read(cx).checkpoint()
        {
            self.model.update(cx, |model, cx| {
                model.toast(
                    format!(
                        "Recovery snapshot failed: {err}. This game is still available in its tab."
                    ),
                    cx,
                )
            });
        }
        if self.active_tab != id {
            // Analyse tabs share the engines, so another Analyse tab always takes them over.
            let keep_analysing = self.model.read(cx).config.analyse_in_background
                && !matches!(tab.content, TabContent::Analyse(_));
            if !keep_analysing {
                self.analyse
                    .update(cx, |screen, cx| screen.set_visible(false, cx));
            }
            self.train
                .update(cx, |screen, cx| screen.set_visible(false, cx));
        }
        if record && self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            self.back_tabs.retain(|visited| *visited != self.active_tab);
            self.back_tabs.push(self.active_tab);
            if self.back_tabs.len() > 50 {
                self.back_tabs.remove(0);
            }
            self.forward_tabs.clear();
        }
        self.active_tab = id;
        self.mode = tab.content.mode();
        self.last_tab.insert(self.mode, id);
        match tab.content {
            TabContent::Analyse(screen) => {
                self.analyse = screen;
                self.analyse
                    .update(cx, |screen, cx| screen.set_visible(true, cx));
            }
            TabContent::Database(screen) => self.database = screen,
            TabContent::Train(screen) => {
                self.train = screen;
                self.train
                    .update(cx, |screen, cx| screen.set_visible(true, cx));
            }
            TabContent::Play(screen) => {
                self.play = screen;
                self.play.update(cx, |screen, cx| screen.resume(cx));
            }
            TabContent::Settings(screen) => self.settings = screen,
            TabContent::Tournament(screen) => self.tournament = Some(screen),
            TabContent::TestPositions(screen) => self.test_positions = Some(screen),
        }
        self.save_session(cx);
        cx.notify();
    }

    pub(super) fn make_tab(
        &self,
        mode: Mode,
        id: u64,
        path: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> WorkspaceTab {
        let model = self.model.clone();
        let workspace = cx.entity().downgrade();
        let directory = model.read(cx).paths.data_dir.join("recovery");
        let content = match mode {
            Mode::Analyse => TabContent::Analyse(cx.new(|cx| {
                AnalyseScreen::new_with_recovery(
                    model,
                    workspace,
                    window,
                    cx,
                    path.unwrap_or_else(|| directory.join(format!("analysis-{id}.json"))),
                )
            })),
            Mode::Play => TabContent::Play(cx.new(|cx| {
                PlayScreen::new_with_recovery(
                    model,
                    workspace,
                    window,
                    cx,
                    path.unwrap_or_else(|| directory.join(format!("play-{id}.json"))),
                )
            })),
            Mode::Database => {
                TabContent::Database(cx.new(|cx| DatabaseScreen::new(model, workspace, window, cx)))
            }
            Mode::Tournament => TabContent::Tournament(
                cx.new(|cx| TournamentScreen::new(model, workspace, window, cx)),
            ),
            Mode::TestPositions => TabContent::TestPositions(
                cx.new(|cx| TestPositionsScreen::new(model, workspace, window, cx)),
            ),
            _ => self
                .existing_content(mode)
                .expect("only optional tabs are created on demand"),
        };
        WorkspaceTab {
            id,
            content,
            name: None,
        }
    }

    pub(super) fn new_tab(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(mode, Mode::Train | Mode::Settings) {
            self.set_mode(mode, cx);
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let recovery_id = self.next_recovery_id.max(id);
        self.next_recovery_id = recovery_id + 1;
        let directory = self.model.read(cx).paths.data_dir.join("recovery");
        let path = match mode {
            Mode::Analyse => Some(directory.join(format!("analysis-{recovery_id}.json"))),
            Mode::Play => Some(directory.join(format!("play-{recovery_id}.json"))),
            _ => None,
        };
        let tab = self.make_tab(mode, id, path, window, cx);
        self.tabs.push(tab);
        self.activate_tab(id, cx);
    }

    pub fn open_analysis(
        &mut self,
        tree: chess_core::GameTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse
            .update(cx, |screen, cx| screen.load_tree(tree, window, cx));
    }

    pub fn open_pgn(&mut self, pgn: &str, ply: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse
            .update(cx, |screen, cx| screen.load_pgn_at(pgn, ply, window, cx));
    }

    /// Opens a stored game in a new Analyse tab, linked to its database so saving
    /// updates it.
    pub fn open_db_game(
        &mut self,
        details: &db::GameDetails,
        ply: usize,
        origin: crate::screens::analyse::DbGame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse.update(cx, |screen, cx| {
            screen.load_db_game(details, ply, origin, window, cx)
        });
    }

    pub(super) fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Analyse(screen) = &self.tabs[index].content {
            if let Err(err) = screen.read(cx).checkpoint() {
                self.model.update(cx, |model, cx| {
                    model.toast(
                        format!("Could not preserve this tab: {err}. Save it before closing."),
                        cx,
                    )
                });
                return;
            }
            screen.update(cx, |screen, cx| screen.set_visible(false, cx));
        }
        if let TabContent::Play(screen) = &self.tabs[index].content {
            screen.update(cx, |screen, cx| screen.suspend(cx));
        }
        let tab = self.tabs.remove(index);
        self.last_tab.retain(|_, value| *value != id);
        self.closed_tabs.push(tab);
        if self.closed_tabs.len() > 10 {
            self.closed_tabs.remove(0);
        }
        if self.active_tab == id {
            if self.tabs.is_empty() {
                self.new_tab(Mode::Analyse, window, cx);
            } else {
                self.activate_tab(self.tabs[index.min(self.tabs.len() - 1)].id, cx);
            }
        }
        self.save_session(cx);
        cx.notify();
    }

    /// Closes every tab safely and starts a fresh Analyse tab numbered one.
    pub(super) fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Preserve every analysis before changing the workspace, just as closing one does.
        for tab in &self.tabs {
            if let TabContent::Analyse(screen) = &tab.content
                && let Err(err) = screen.read(cx).checkpoint()
            {
                self.model.toast(
                    format!("Could not preserve this tab: {err}. Save it before closing."),
                    cx,
                );
                return;
            }
        }
        for tab in &self.tabs {
            match &tab.content {
                TabContent::Analyse(screen) => {
                    screen.update(cx, |screen, cx| screen.set_visible(false, cx))
                }
                TabContent::Play(screen) => screen.update(cx, |screen, cx| screen.suspend(cx)),
                TabContent::Train(screen) => {
                    screen.update(cx, |screen, cx| screen.set_visible(false, cx))
                }
                _ => {}
            }
        }
        self.closed_tabs.append(&mut self.tabs);
        if self.closed_tabs.len() > 10 {
            self.closed_tabs.drain(..self.closed_tabs.len() - 10);
        }
        // Reopened tabs receive fresh numbers, while retaining their original recovery files.
        for tab in &mut self.closed_tabs {
            tab.id = 0;
        }
        self.last_tab.clear();
        self.back_tabs.clear();
        self.forward_tabs.clear();
        self.tab_watch.clear();
        self.renaming = None;
        self.active_tab = u64::MAX;
        self.next_recovery_id = self.next_recovery_id.max(self.next_tab_id);
        self.next_tab_id = 1;
        self.tab_in_view.set(0);
        self.new_tab(Mode::Analyse, window, cx);
    }

    pub(super) fn reopen_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(mut tab) = self.closed_tabs.pop() {
            if tab.id == 0 {
                tab.id = self.next_tab_id;
                self.next_tab_id += 1;
            }
            let id = tab.id;
            self.tabs.push(tab);
            self.activate_tab(id, cx);
        }
    }

    pub(super) fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0);
        let index = (current as isize + delta).rem_euclid(self.tabs.len() as isize) as usize;
        self.activate_tab(self.tabs[index].id, cx);
    }

    /// Opens a field in the tab's place to type a new name for it.
    pub(super) fn start_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let current = match &tab.name {
            Some(name) => name.to_string(),
            None => tab.content.title(cx).0.to_owned(),
        };
        self.renaming = Some(crate::controls::Rename::start(
            id,
            current,
            window,
            cx,
            |this, window, cx| this.finish_rename(true, window, cx),
        ));
        cx.notify();
    }

    /// Closes the rename field, keeping the typed name when `keep` is set. An empty
    /// name gives the tab back its usual title.
    pub(super) fn finish_rename(
        &mut self,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let id = rename.key;
        if keep {
            let typed = rename.text(cx);
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                let default = tab.content.title(cx).0;
                tab.name = (!typed.is_empty() && typed != default).then(|| typed.into());
            }
            self.save_session(cx);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Starts watching tabs opened since the last frame and forgets closed ones.
    pub(super) fn watch_tabs(&mut self, cx: &mut Context<Self>) {
        self.tab_watch
            .retain(|id, _| self.tabs.iter().any(|tab| tab.id == *id));
        for tab in &self.tabs {
            if self.tab_watch.contains_key(&tab.id) {
                continue;
            }
            let id = tab.id;
            let observe = move |this: &mut Self, cx: &mut Context<Self>| {
                let now = this
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .and_then(|tab| tab.content.activity(cx));
                if let Some(entry) = this.tab_watch.get_mut(&id)
                    && entry.0 != now
                {
                    entry.0 = now;
                    cx.notify();
                }
            };
            let subscription = match &tab.content {
                TabContent::Analyse(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::Database(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::Train(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::Play(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::Settings(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::Tournament(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
                TabContent::TestPositions(e) => cx.observe(e, move |this, _, cx| observe(this, cx)),
            };
            let now = tab.content.activity(cx);
            self.tab_watch.insert(id, (now, subscription));
        }
    }
}
