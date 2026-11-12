//! Saving the open tabs when the app closes and restoring them, or the recovered work, at start.

use super::*;

impl Workspace {
    pub(super) fn restore_recovery_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self.model.read(cx).paths.data_dir.join("recovery");
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            let mode = if name.starts_with("analysis-") {
                Mode::Analyse
            } else if name.starts_with("play-") {
                Mode::Play
            } else {
                continue;
            };
            let Some(id) = name
                .rsplit('-')
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|id| *id > 1 && *id < u64::MAX)
            else {
                continue;
            };
            if self.tabs.iter().any(|tab| tab.id == id)
                || crate::recovery::Recovery::load(&path).is_none()
            {
                continue;
            }
            self.next_tab_id = self.next_tab_id.max(id + 1);
            self.next_recovery_id = self.next_recovery_id.max(id + 1);
            let tab = self.make_tab(mode, id, Some(path), window, cx);
            self.tabs.push(tab);
        }
    }

    /// Reopens the tabs that were open when Fast last closed, in their order and with
    /// their names, and shows the one that was shown.
    pub(super) fn restore_session(
        &mut self,
        session: crate::config::SavedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directory = self.model.read(cx).paths.data_dir.join("recovery");
        let default_analyse = self.analyse.read(cx).recovery_path().to_path_buf();
        let default_play = self.play.read(cx).recovery_path().to_path_buf();
        // The first tab of each kind takes the screen made at start; later ones get their own.
        let mut used: Vec<Mode> = Vec::new();
        let mut tabs: Vec<WorkspaceTab> = Vec::new();
        for saved in &session.tabs {
            let mode = Mode::from_saved(saved.kind);
            if saved.id == 0 || saved.id == u64::MAX || tabs.iter().any(|tab| tab.id == saved.id) {
                continue;
            }
            // Only a bare file name is accepted, so a damaged config cannot point elsewhere.
            let path = saved
                .recovery
                .as_deref()
                .and_then(|name| std::path::Path::new(name).file_name())
                .map(|name| {
                    let default = match mode {
                        Mode::Analyse => &default_analyse,
                        _ => &default_play,
                    };
                    if default.file_name() == Some(name) {
                        default.clone()
                    } else {
                        directory.join(name)
                    }
                });
            let first = !used.contains(&mode);
            let reuse = match mode {
                Mode::Analyse => first && path.as_ref() == Some(&default_analyse),
                Mode::Play => first && path.as_ref() == Some(&default_play),
                Mode::Database | Mode::Train | Mode::Settings => first,
                Mode::Tournament | Mode::TestPositions => false,
            };
            if matches!(mode, Mode::Train | Mode::Settings) && !first {
                continue;
            }
            let mut tab = match reuse.then(|| self.existing_content(mode)).flatten() {
                Some(content) => WorkspaceTab {
                    id: saved.id,
                    content,
                    name: None,
                },
                None => self.make_tab(mode, saved.id, path, window, cx),
            };
            if let TabContent::Analyse(screen) = &tab.content {
                let choice = saved.explorer_db.clone();
                screen.update(cx, |screen, cx| screen.restore_explorer_choice(choice, cx));
            }
            tab.name = saved
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| SharedString::from(name.to_owned()));
            used.push(mode);
            tabs.push(tab);
        }
        if tabs.is_empty() {
            return;
        }
        // New tabs must not pick up a recovery file left by a tab closed earlier.
        let highest_file = std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let stem = std::path::Path::new(&name)
                    .file_stem()?
                    .to_str()?
                    .to_owned();
                stem.rsplit('-').next()?.parse::<u64>().ok()
            })
            .filter(|id| *id < u64::MAX)
            .max()
            .unwrap_or(0);
        let highest_tab = tabs.iter().map(|tab| tab.id).max().unwrap_or(0);
        self.next_tab_id = highest_tab + 1;
        self.next_recovery_id = highest_file.max(highest_tab) + 1;
        let active = tabs
            .get(session.active)
            .or(tabs.first())
            .map(|tab| tab.id)
            .unwrap_or(1);
        self.last_tab = tabs
            .iter()
            .map(|tab| (tab.content.mode(), tab.id))
            .collect();
        self.tabs = tabs;
        // The screens made at start may not be in any tab, so none keeps running unseen.
        self.analyse
            .update(cx, |screen, cx| screen.set_visible(false, cx));
        self.train
            .update(cx, |screen, cx| screen.set_visible(false, cx));
        self.active_tab = u64::MAX;
        self.activate_tab(active, cx);
    }

    /// Records the open tabs in the config, so the next start reopens them.
    pub(super) fn save_session(&self, cx: &mut Context<Self>) {
        let file_name = |path: &std::path::Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        };
        let tabs = self
            .tabs
            .iter()
            .map(|tab| crate::config::SavedTab {
                kind: tab.content.mode().saved(),
                id: tab.id,
                name: tab.name.as_ref().map(|name| name.to_string()),
                recovery: match &tab.content {
                    TabContent::Analyse(screen) => file_name(screen.read(cx).recovery_path()),
                    TabContent::Play(screen) => file_name(screen.read(cx).recovery_path()),
                    _ => None,
                },
                explorer_db: match &tab.content {
                    TabContent::Analyse(screen) => screen
                        .read(cx)
                        .explorer_choice()
                        .map(|path| path.to_path_buf()),
                    _ => None,
                },
            })
            .collect();
        let active = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0);
        let session = Some(crate::config::SavedSession { tabs, active });
        self.model.update(cx, |model, cx| {
            if model.config.session != session {
                model.config.session = session;
                model.schedule_config_save(cx);
            }
        });
    }
}
