//! Running a tournament: launching fastchess, following its output and stopping it.

use super::*;

impl TournamentScreen {
    /// Continues from a saved config, in the folder the tournament ran in.
    pub(super) fn resume(
        &mut self,
        dir: PathBuf,
        cwd: PathBuf,
        config: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.launch(
            Launch {
                cwd,
                args: fastchess::resume_args(&config),
                dir,
                config,
            },
            cx,
        );
    }

    pub(super) fn launch(&mut self, launch: Launch, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(Run::active) {
            return self.fail("A tournament is already running in this tab", cx);
        }
        self.form_error = None;
        match self.program(cx) {
            Some(program) => self.spawn(program, launch, cx),
            None => {
                self.pending = Some(launch);
                self.download(cx);
            }
        }
        cx.notify();
    }

    pub(super) fn spawn(&mut self, program: PathBuf, launch: Launch, cx: &mut Context<Self>) {
        let pgn_offset = std::fs::read_to_string(launch.dir.join("games.pgn"))
            .map(|text| fastchess::count_games(&text))
            .unwrap_or(0);
        let previous = fastchess::saved_games(&launch.config);
        let has_events = *self
            .events_support
            .entry(program.clone())
            .or_insert_with(|| tournament_live::has_events(&program));
        let live_off = if !has_events {
            Some(ui::i18n::t(
                "Live boards need a fastchess build with the -events option.",
            ))
        } else if tournament_live::has_own_events(&launch.args) {
            Some(ui::i18n::t(
                "Live boards are off because this command sets its own -events option.",
            ))
        } else {
            None
        };
        let live = live_off
            .is_none()
            .then(|| Arc::new(Mutex::new(LiveGames::default())));
        // The event options are Fast's business: they stay out of the command shown to copy.
        let mut args = launch.args.clone();
        if live.is_some() {
            args.extend(tournament_live::events_args());
        }
        let process = match fastchess::spawn(&program, &args, &launch.cwd, live.clone()) {
            Ok(process) => process,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let stop = process.stop_handle();
        let names = engines(&launch.args)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let concurrency = flag_value(&launch.args, "-concurrency")
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(fastchess::default_concurrency);
        let task = cx.spawn(async move |this, cx| {
            while let Ok(first) = process.lines.recv().await {
                // Take whatever else is waiting so a burst costs one redraw.
                let mut lines = vec![first];
                while let Ok(line) = process.lines.try_recv() {
                    lines.push(line);
                }
                let alive = this.update(cx, |screen, cx| {
                    if let Some(run) = screen.run.as_mut() {
                        for line in lines {
                            run.handle(line);
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let exit = cx.background_spawn(async move { process.wait() }).await;
            let _ = this.update(cx, |screen, cx| {
                screen.finish(exit, cx);
            });
        });
        self.run = Some(Run {
            dir: launch.dir,
            cwd: launch.cwd,
            config: launch.config,
            program,
            args: launch.args,
            status: Status::Running,
            stop: Some(stop),
            total: 0,
            finished: 0,
            previous,
            pgn_offset,
            games: VecDeque::new(),
            block: Vec::new(),
            in_block: false,
            results: Vec::new(),
            log: VecDeque::new(),
            _live_task: live.is_some().then(|| {
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(LIVE_REFRESH).await;
                        match this.update(cx, |screen, cx| screen.refresh_live(cx)) {
                            Ok(true) => {}
                            _ => break,
                        }
                    }
                })
            }),
            live,
            live_off,
            live_seen: 0,
            names,
            concurrency,
            started: HashMap::new(),
            game_time: (Duration::ZERO, 0),
            _task: task,
        });
        self.live_games.clear();
        self.live_boards.clear();
        self.live_focus = None;
        self.setup_open = false;
        cx.notify();
    }

    /// Copies new moves from the event stream into the boards. Returns false once the
    /// run is over, which ends the refresh loop.
    pub(super) fn refresh_live(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.run.as_mut() else {
            return false;
        };
        if !run.active() {
            return false;
        }
        let Some(live) = &run.live else {
            return false;
        };
        let games = {
            let Ok(live) = live.lock() else {
                return false;
            };
            if live.serial == run.live_seen {
                return true;
            }
            run.live_seen = live.serial;
            live.snapshot()
        };
        let prefs = self.model.read(cx).config.board.prefs();
        for game in &games {
            let entry = self.live_boards.entry(game.slot).or_insert_with(|| {
                let board = cx.new(|_| BoardView::new(("live-board", game.slot)).read_only());
                (board, 0, usize::MAX)
            });
            let mut shown = (entry.1, entry.2);
            tournament_wall::sync_board(&entry.0, &mut shown, game, prefs, cx);
            (entry.1, entry.2) = shown;
        }
        if let Some(game) = self
            .live_focus
            .and_then(|slot| games.iter().find(|game| game.slot == slot))
        {
            let board = self.focus_board.clone();
            let mut shown = self.focus_shown;
            tournament_wall::sync_board(&board, &mut shown, game, prefs, cx);
            self.focus_shown = shown;
        }
        self.live_games = games;
        cx.notify();
        true
    }

    pub(super) fn focus_live(&mut self, slot: usize, cx: &mut Context<Self>) {
        self.live_focus = (self.live_focus != Some(slot)).then_some(slot);
        self.focus_shown = (0, usize::MAX);
        if let Some(run) = self.run.as_mut() {
            // Draw the focused board on the next refresh even if nothing moved.
            run.live_seen = run.live_seen.wrapping_sub(1);
        }
        self.refresh_live(cx);
        cx.notify();
    }

    pub(super) fn finish(&mut self, exit: fastchess::Exit, cx: &mut Context<Self>) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        run.stop = None;
        let resumable = run.config.is_file();
        run.status = if exit.stopped {
            Status::Finished(if resumable {
                "Stopped. Continue picks up where it left off.".into()
            } else {
                "Stopped.".into()
            })
        } else if exit.success {
            Status::Finished("Finished.".into())
        } else {
            let last = run
                .log
                .iter()
                .rev()
                .find(|line| !line.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            Status::Failed(match exit.code {
                Some(code) => format!("fastchess exited with code {code}. {last}"),
                None => format!("fastchess was terminated. {last}"),
            })
        };
        // The games in progress ended with the run.
        self.live_games.clear();
        self.live_focus = None;
        self.saved = fastchess::saved_runs(&self.data_dir(cx));
        self.measure_folder(cx);
        cx.notify();
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.run.as_mut()
            && let Some(stop) = &run.stop
        {
            stop.stop();
            run.status = Status::Stopping;
            cx.notify();
        }
    }

    /// Opens fastchess game `number` in Analyse: by its `GameId` tag when fastchess wrote
    /// one, otherwise by its place in the file (games are written in finishing order).
    pub(super) fn open_game(
        &mut self,
        number: u64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = &self.run else { return };
        let game = std::fs::read_to_string(run.pgn_file())
            .ok()
            .and_then(|text| {
                if text.contains("[GameId ") {
                    fastchess::game_by_id(&text, number)
                } else {
                    fastchess::nth_game(&text, index)
                }
            });
        match game {
            Some(pgn) => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| workspace.open_pgn(&pgn, 0, window, cx));
                }
            }
            None => self
                .model
                .toast("That game is not in the PGN file yet.", cx),
        }
    }

    pub(super) fn copy_command(&self, cx: &mut Context<Self>) {
        if let Some(run) = &self.run {
            let command = fastchess::display_command(&run.program.display().to_string(), &run.args);
            cx.write_to_clipboard(ClipboardItem::new_string(command));
            self.model.toast("Command copied", cx);
        }
    }

    pub(super) fn copy_config(&self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else { return };
        match std::fs::read_to_string(&run.config) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.model.toast("config.json copied", cx);
            }
            Err(_) => self
                .model
                .toast("fastchess has not saved a config yet.", cx),
        }
    }

    /// Opens the watch-all window on the games in progress.
    pub(super) fn watch_all(&self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else { return };
        let Some(live) = run.live.clone() else { return };
        LiveWall::open(self.model.clone(), live, run.names.clone(), cx);
    }
}
