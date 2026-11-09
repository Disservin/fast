//! Test positions: load an EPD suite, run engines over it looking for the best move or
//! the mate, and compare how many each one solves.

use std::{
    ops::Range,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use chess_core::{EpdLine, EpdPosition};
use engine::EngineId;
use gpui::{
    Context, Div, Entity, ExternalPaths, FontWeight, IntoElement, Render, SharedString, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::*, px, relative, rgb, uniform_list,
};
use gpui_component::input::InputState;
use ui::{
    MONO, T,
    widgets::{btn, check, h_flex, section_label, segmented, v_flex},
};

use crate::{
    config::{TestGoal, TestLimitKind, TestPositionSettings},
    controls::{field, new_input, text_input},
    model::{AppModel, pick_directory, pick_path},
    shell::Workspace,
    test_suite::{self, Cell, RunEvent, RunSettings, Search},
};

const ROW_H: f32 = 34.;
const W_LINE: f32 = 52.;
const W_ID: f32 = 130.;
const W_EXPECT: f32 = 150.;

/// One run: which positions and engines, and every result so far.
struct Run {
    engines: Vec<String>,
    /// For each file line, its index among the run's positions.
    position_of: Vec<Option<usize>>,
    positions: usize,
    /// `cells[engine][position]`.
    cells: Vec<Vec<Cell>>,
    goal: TestGoal,
    cancel: Arc<AtomicBool>,
    started: Instant,
    finished: Option<Duration>,
    _reader: Task<()>,
    /// Redraws the elapsed time while the run is active.
    _ticker: Task<()>,
}

impl Run {
    fn active(&self) -> bool {
        self.finished.is_none()
    }

    fn done(&self) -> usize {
        self.cells
            .iter()
            .map(|column| column.iter().filter(|cell| cell.done()).count())
            .sum()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct TestPositionsScreen {
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    file: Option<PathBuf>,
    lines: Vec<EpdLine>,
    load_error: Option<SharedString>,
    settings: TestPositionSettings,
    limit_value: Entity<InputState>,
    hash: Entity<InputState>,
    processes: Entity<InputState>,
    syzygy_path: Entity<InputState>,
    form_error: Option<SharedString>,
    run: Option<Run>,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
}

impl TestPositionsScreen {
    /// Engines are working through the suite.
    pub fn is_running(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.finished.is_none())
    }

    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = model.read(cx).config.test_positions.clone();
        let known: Vec<EngineId> = model.read(cx).config.engines.iter().map(|e| e.id).collect();
        settings.engines.retain(|id| known.contains(id));
        if settings.engines.is_empty()
            && let Some(first) = known.first()
        {
            settings.engines.push(*first);
        }
        cx.on_app_quit(|screen: &mut Self, _| {
            if let Some(run) = &screen.run {
                run.cancel.store(true, Ordering::Relaxed);
            }
            async {}
        })
        .detach();
        Self {
            limit_value: new_input(limit_text(&settings), "", window, cx),
            hash: new_input(
                settings
                    .hash_mb
                    .map(|mb| mb.to_string())
                    .unwrap_or_default(),
                "Engine setting",
                window,
                cx,
            ),
            processes: new_input(settings.processes.max(1).to_string(), "1", window, cx),
            syzygy_path: new_input(
                settings
                    .syzygy_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                "Folder with .rtbw and .rtbz files",
                window,
                cx,
            ),
            model,
            workspace,
            file: None,
            lines: Vec::new(),
            load_error: None,
            settings,
            form_error: None,
            run: None,
            scroll: UniformListScrollHandle::new(),
            _load: None,
        }
    }

    /// Progress beside the tab's name.
    pub fn tab_detail(&self) -> Option<SharedString> {
        if let Some(run) = &self.run {
            let total = run.positions * run.engines.len();
            return Some(if run.active() {
                format!("{}/{}", run.done(), total).into()
            } else {
                "finished".into()
            });
        }
        self.file
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned().into())
    }

    pub fn breadcrumb(&self) -> SharedString {
        match self.tab_detail() {
            Some(detail) => format!("Test positions, {detail}").into(),
            None => ui::i18n::t("Test positions").into(),
        }
    }

    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(Run::active)
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Test positions (.epd)",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |screen, cx| screen.load(path, cx));
            },
        );
    }

    fn choose_syzygy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let window_handle = window.window_handle();
        pick_directory(
            self.model.clone(),
            "Syzygy tablebases",
            cx,
            move |path, cx| {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |screen, cx| {
                        screen.syzygy_path.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx)
                        });
                        screen.settings.syzygy = true;
                        cx.notify();
                    });
                });
            },
        );
    }

    /// Reads and parses a file in the background.
    fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.running() {
            return;
        }
        let read = path.clone();
        self._load = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(&read)?;
                    Ok::<_, std::io::Error>(chess_core::parse_epd(&String::from_utf8_lossy(&bytes)))
                })
                .await;
            let _ = this.update(cx, |screen, cx| {
                match parsed {
                    Ok(lines) => {
                        screen.lines = lines;
                        screen.load_error =
                            (screen.lines.is_empty()).then(|| "The file has no positions.".into());
                        screen.file = Some(path);
                        screen.run = None;
                    }
                    Err(err) => {
                        screen.load_error =
                            Some(format!("Could not read {}: {err}", path.display()).into());
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Why a line is skipped for the chosen goal, if it is.
    fn problem(&self, line: &EpdLine) -> Option<SharedString> {
        match &line.parsed {
            Err(err) => Some(err.clone().into()),
            Ok(record) => test_suite::unusable(record, self.settings.goal).map(Into::into),
        }
    }

    fn counts(&self) -> (usize, usize) {
        let skipped = self
            .lines
            .iter()
            .filter(|line| self.problem(line).is_some())
            .count();
        (self.lines.len() - skipped, skipped)
    }

    /// Reads the text fields into the settings, or says what is wrong.
    fn read_form(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let text = |state: &Entity<InputState>| state.read(cx).value().trim().to_owned();
        let limit = text(&self.limit_value).replace(['_', ','], "");
        match self.settings.limit {
            TestLimitKind::Depth => {
                self.settings.depth = limit
                    .parse::<u32>()
                    .ok()
                    .filter(|depth| *depth > 0)
                    .ok_or("Depth must be a whole number above 0.")?;
            }
            TestLimitKind::Nodes => {
                self.settings.nodes = parse_count(&limit)
                    .filter(|nodes| *nodes > 0)
                    .ok_or("Nodes must be a number above 0, such as 500000 or 2M.")?;
            }
            TestLimitKind::Time => {
                self.settings.time_ms = limit
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| *seconds > 0.)
                    .map(|seconds| (seconds * 1000.).round() as u64)
                    .ok_or("Time must be a number of seconds above 0, such as 0.5.")?;
            }
        }
        let hash = text(&self.hash);
        self.settings.hash_mb = if hash.is_empty() {
            None
        } else {
            Some(
                hash.parse::<u32>()
                    .ok()
                    .filter(|mb| *mb > 0)
                    .ok_or("Hash must be a number of MB, or empty to keep each engine's.")?,
            )
        };
        if self.settings.parallel {
            self.settings.processes = text(&self.processes)
                .parse::<u32>()
                .ok()
                .filter(|count| (1..=256).contains(count))
                .ok_or("Processes per engine must be between 1 and 256.")?;
        }
        let syzygy = text(&self.syzygy_path);
        self.settings.syzygy_path = (!syzygy.is_empty()).then(|| PathBuf::from(syzygy));
        if self.settings.syzygy {
            match &self.settings.syzygy_path {
                None => return Err("Choose the folder with your Syzygy tablebases.".into()),
                Some(path) if !path.is_dir() => {
                    return Err(format!("{} is not a folder.", path.display()));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running() {
            return;
        }
        if let Err(err) = self.read_form(cx) {
            self.form_error = Some(err.into());
            cx.notify();
            return;
        }
        let engines: Vec<engine::EngineConfig> = {
            let config = &self.model.read(cx).config;
            self.settings
                .engines
                .iter()
                .filter_map(|id| config.engines.iter().find(|e| e.id == *id).cloned())
                .collect()
        };
        if engines.is_empty() {
            self.form_error = Some(ui::i18n::t("Choose at least one engine.").into());
            cx.notify();
            return;
        }
        let mut position_of = Vec::with_capacity(self.lines.len());
        let mut positions: Vec<EpdPosition> = Vec::new();
        for line in &self.lines {
            match (&line.parsed, self.problem(line)) {
                (Ok(record), None) => {
                    position_of.push(Some(positions.len()));
                    positions.push(record.clone());
                }
                _ => position_of.push(None),
            }
        }
        if positions.is_empty() {
            self.form_error = Some(
                match self.settings.goal {
                    TestGoal::BestMove => "No position has a bm or am operation to check.",
                    TestGoal::Mate => "No position has a dm operation to check.",
                }
                .into(),
            );
            cx.notify();
            return;
        }
        self.form_error = None;
        let saved = self.settings.clone();
        self.model.update(cx, |model, cx| {
            model.config.test_positions = saved.clone();
            model.schedule_config_save(cx);
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = smol::channel::unbounded();
        let count = positions.len();
        test_suite::start(
            engines.clone(),
            Arc::new(positions),
            RunSettings::from_saved(&saved),
            cancel.clone(),
            tx,
        );
        let reader = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Ok(first) = rx.recv().await {
                // Take everything already queued, then draw once.
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                let alive = this.update(cx, |screen, cx| {
                    let Some(run) = &mut screen.run else { return };
                    for event in batch {
                        match event {
                            RunEvent::Cell {
                                engine,
                                position,
                                cell,
                            } => {
                                if let Some(slot) = run
                                    .cells
                                    .get_mut(engine)
                                    .and_then(|column| column.get_mut(position))
                                {
                                    *slot = cell;
                                }
                            }
                            RunEvent::Finished => {
                                run.finished = Some(run.started.elapsed());
                            }
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        let ticker = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let active = this
                    .update(cx, |screen, cx| {
                        cx.notify();
                        screen.running()
                    })
                    .unwrap_or(false);
                if !active {
                    return;
                }
            }
        });
        self.run = Some(Run {
            engines: engines.iter().map(|e| e.name.clone()).collect(),
            position_of,
            positions: count,
            cells: vec![vec![Cell::Waiting; count]; engines.len()],
            goal: saved.goal,
            cancel,
            started: Instant::now(),
            finished: None,
            _reader: reader,
            _ticker: ticker,
        });
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &self.run {
            run.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    fn open_line(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(record)) = self.lines.get(index).map(|line| &line.parsed) else {
            return;
        };
        let Ok(tree) = chess_core::GameTree::from_fen(&record.fen) else {
            return;
        };
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.open_analysis(tree, window, cx)
            });
        }
    }

    fn toggle<F>(
        &self,
        id: &'static str,
        on: bool,
        label: &'static str,
        cx: &mut Context<Self>,
        set: F,
    ) -> impl IntoElement + use<F>
    where
        F: Fn(&mut Self, bool) + 'static,
    {
        let this = cx.entity();
        check(id, on, label).text(13.).on_change(move |on, _, cx| {
            this.update(cx, |screen, cx| {
                set(screen, on);
                cx.notify();
            })
        })
    }

    fn render_file(&self, cx: &mut Context<Self>) -> Div {
        let locked = self.running();
        let choose = cx.entity();
        let (usable, skipped) = self.counts();
        let summary =
            self.file.as_ref().map(|path| {
                v_flex()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(SharedString::from(
                                path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .text_size(px(12.5))
                            .child(div().text_color(rgb(T.fg_2)).child(SharedString::from(
                                format!("{usable} position{}", if usable == 1 { "" } else { "s" }),
                            )))
                            .when(skipped > 0, |row| {
                                row.child(
                                    div()
                                        .text_color(rgb(T.danger_fg))
                                        .child(SharedString::from(format!("{skipped} skipped"))),
                                )
                            }),
                    )
            });
        let drop = cx.entity();
        v_flex()
            .gap(px(10.))
            .child(section_label("Positions"))
            .child(
                v_flex()
                    .id("epd-drop")
                    .gap(px(8.))
                    .items_center()
                    .justify_center()
                    .min_h(px(112.))
                    .p(px(16.))
                    .rounded(px(10.))
                    .border_2()
                    .border_dashed()
                    .border_color(rgb(T.border_strong))
                    .bg(rgb(T.bg))
                    .drag_over::<ExternalPaths>(|style, _, _, _| {
                        style.border_color(rgb(T.accent)).bg(rgb(T.selected))
                    })
                    .on_drop(move |paths: &ExternalPaths, _, cx| {
                        let Some(path) = paths.paths().first().cloned() else {
                            return;
                        };
                        drop.update(cx, |screen, cx| screen.load(path, cx));
                    })
                    .children(summary)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(T.muted))
                            .text_center()
                            .child(ui::i18n::t("Drop an .epd file here")),
                    )
                    .child(
                        btn("epd-choose", ui::i18n::t("Choose file…"))
                            .disabled(locked)
                            .on_click(move |_, cx| {
                                choose.update(cx, |screen, cx| screen.choose_file(cx))
                            }),
                    ),
            )
            .children(self.load_error.clone().map(|err| {
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(T.danger_fg))
                    .child(err)
            }))
    }

    fn render_engines(&self, cx: &mut Context<Self>) -> Div {
        let engines = self.model.read(cx).config.engines.clone();
        let mut list = v_flex().gap(px(6.));
        if engines.is_empty() {
            list = list.child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                ui::i18n::t("No engines yet. Add them in Settings › Engines."),
            ));
        }
        for engine in engines {
            let id = engine.id;
            let this = cx.entity();
            list = list.child(
                check(
                    ("test-engine", id as usize),
                    self.settings.engines.contains(&id),
                    engine.name.clone(),
                )
                .text(13.)
                .on_change(move |on, _, cx| {
                    this.update(cx, |screen, cx| {
                        screen.settings.engines.retain(|e| *e != id);
                        if on {
                            screen.settings.engines.push(id);
                        }
                        cx.notify();
                    })
                }),
            );
        }
        v_flex()
            .gap(px(8.))
            .child(section_label("Engines"))
            .child(list)
    }

    fn render_form(&self, cx: &mut Context<Self>) -> Div {
        let goal_this = cx.entity();
        let limit_this = cx.entity();
        let browse = cx.entity();
        let start = cx.entity();
        let stop = cx.entity();
        let running = self.running();
        let limit_hint = match self.settings.limit {
            TestLimitKind::Depth => "Depth per position",
            TestLimitKind::Nodes => "Nodes per position (500000, 2M)",
            TestLimitKind::Time => "Seconds per position",
        };
        let mate_note = (self.settings.goal == TestGoal::Mate).then(|| {
            div()
                .text_size(px(12.))
                .text_color(rgb(T.muted))
                .child(ui::i18n::t(
                    "Engines search with go mate N from each dm, stopping at the limit below if they have not found it. A shorter mate counts too.",
                ))
        });
        let pair = |a: Div, b: Div| {
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(a.flex_1())
                .child(b.flex_1())
        };
        v_flex()
            .gap(px(14.))
            .child(section_label("Find"))
            .child(segmented(
                "test-goal",
                vec![
                    (TestGoal::BestMove, ui::i18n::t("Best move").into(), running),
                    (TestGoal::Mate, ui::i18n::t("Mate").into(), running),
                ],
                self.settings.goal,
                move |goal, _, cx| {
                    goal_this.update(cx, |screen, cx| {
                        screen.settings.goal = goal;
                        cx.notify();
                    })
                },
            ))
            .children(mate_note)
            .child(self.render_engines(cx))
            .child(section_label("Search limit"))
            .child(segmented(
                "test-limit",
                vec![
                    (TestLimitKind::Depth, ui::i18n::t("Depth").into(), running),
                    (TestLimitKind::Nodes, ui::i18n::t("Nodes").into(), running),
                    (TestLimitKind::Time, ui::i18n::t("Time").into(), running),
                ],
                self.settings.limit,
                move |limit, window, cx| {
                    limit_this.update(cx, |screen, cx| {
                        // Keep what was typed for the old kind, show the new kind's value.
                        let _ = screen.read_form(cx);
                        screen.settings.limit = limit;
                        let text = limit_text(&screen.settings);
                        screen
                            .limit_value
                            .update(cx, |input, cx| input.set_value(text, window, cx));
                        cx.notify();
                    })
                },
            ))
            .child(field(limit_hint, text_input(&self.limit_value)))
            .child(section_label("Hash"))
            .child(field(
                "Hash (MB)",
                text_input(&self.hash).suffix(Some(crate::controls::hash_suffix(
                    &self.hash.read(cx).value(),
                ))),
            ))
            .child(self.toggle(
                "test-clear-hash",
                self.settings.clear_hash,
                "Clear hash between positions",
                cx,
                |screen, on| screen.settings.clear_hash = on,
            ))
            .child(section_label("Running"))
            .child(self.toggle(
                "test-parallel",
                self.settings.parallel,
                "Run in parallel",
                cx,
                |screen, on| screen.settings.parallel = on,
            ))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(if self.settings.parallel {
                        "All engines run at the same time, each split over this many processes."
                    } else {
                        "One engine process at a time, engine after engine."
                    })),
            )
            .when(self.settings.parallel, |column| {
                column.child(pair(
                    field("Processes per engine", text_input(&self.processes)),
                    div(),
                ))
            })
            .child(section_label("Tablebases"))
            .child(self.toggle(
                "test-syzygy",
                self.settings.syzygy,
                "Use Syzygy tablebases",
                cx,
                |screen, on| screen.settings.syzygy = on,
            ))
            .when(self.settings.syzygy, |column| {
                column.child(field(
                    "Syzygy folder",
                    h_flex()
                        .gap(px(6.))
                        .child(div().flex_1().child(text_input(&self.syzygy_path)))
                        .child(btn("test-syzygy-browse", ui::i18n::t("Browse…")).on_click(
                            move |window, cx| {
                                browse.update(cx, |screen, cx| screen.choose_syzygy(window, cx))
                            },
                        )),
                ))
            })
            .children(self.form_error.clone().map(|err| {
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(T.danger_fg))
                    .child(err)
            }))
            .child(if running {
                btn("test-stop", ui::i18n::t("Stop"))
                    .h(38.)
                    .full()
                    .on_click(move |_, cx| stop.update(cx, |screen, cx| screen.stop(cx)))
            } else {
                btn("test-start", ui::i18n::t("Run test"))
                    .primary()
                    .h(38.)
                    .full()
                    .disabled(self.lines.is_empty())
                    .on_click(move |_, cx| start.update(cx, |screen, cx| screen.start(cx)))
            })
    }

    fn render_summary(&self, run: &Run) -> Div {
        let mut tiles = h_flex().gap(px(12.)).flex_wrap();
        for (slot, name) in run.engines.iter().enumerate() {
            let tally = test_suite::tally(&run.cells[slot]);
            let share = if tally.done == 0 {
                0.
            } else {
                tally.solved as f32 / tally.done as f32
            };
            tiles =
                tiles.child(
                    v_flex()
                        .min_w(px(200.))
                        .flex_1()
                        .gap(px(6.))
                        .p(px(14.))
                        .rounded(px(10.))
                        .bg(rgb(T.tour_tile))
                        .border_1()
                        .border_color(rgb(T.tour_tile_border))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .items_baseline()
                                .child(
                                    div()
                                        .text_size(px(26.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(SharedString::from(format!(
                                            "{}/{}",
                                            tally.solved, run.positions
                                        ))),
                                )
                                .child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                                    SharedString::from(format!("{:.1}% of run", share * 100.)),
                                )),
                        )
                        .child(meter(
                            tally.done as f32 / run.positions.max(1) as f32,
                            T.good,
                        ))
                        .child(
                            h_flex()
                                .justify_between()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted))
                                .child(SharedString::from(format!(
                                    "{} of {} run",
                                    tally.done, run.positions
                                )))
                                .child(
                                    div()
                                        .font_family(MONO)
                                        .child(SharedString::from(format_ms(tally.time_ms))),
                                ),
                        ),
                );
        }
        let status = match run.finished {
            Some(took) if run.cancel.load(Ordering::Relaxed) => {
                format!("Stopped after {}", format_ms(took.as_millis() as u64))
            }
            Some(took) => format!("Finished in {}", format_ms(took.as_millis() as u64)),
            None if run.cancel.load(Ordering::Relaxed) => "Stopping…".to_owned(),
            None => format!(
                "Running for {}",
                format_ms(run.started.elapsed().as_millis() as u64)
            ),
        };
        v_flex()
            .gap(px(10.))
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label(match run.goal {
                        TestGoal::BestMove => "Best move results",
                        TestGoal::Mate => "Mate results",
                    }))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(status)),
                    ),
            )
            .child(tiles)
    }

    fn render_table(&self, cx: &mut Context<Self>) -> Div {
        let engines: Vec<String> = match &self.run {
            Some(run) => run.engines.clone(),
            None => {
                let config = &self.model.read(cx).config;
                self.settings
                    .engines
                    .iter()
                    .filter_map(|id| config.engines.iter().find(|e| e.id == *id))
                    .map(|e| e.name.clone())
                    .collect()
            }
        };
        let mut header = h_flex()
            .h(px(32.))
            .px(px(12.))
            .gap(px(8.))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(div().w(px(W_LINE)).flex_none().child(section_label("Line")))
            .child(div().w(px(W_ID)).flex_none().child(section_label("ID")))
            .child(
                div()
                    .w(px(W_EXPECT))
                    .flex_none()
                    .child(section_label("Expected")),
            );
        for name in &engines {
            header = header.child(
                div()
                    .flex_1()
                    .min_w(px(140.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(T.muted))
                    .child(SharedString::from(name.clone())),
            );
        }
        let count = self.lines.len();
        let list = uniform_list(
            "test-positions-table",
            count,
            cx.processor(|this, range: Range<usize>, _window, cx| {
                range
                    .map(|index| this.render_row(index, cx).into_any_element())
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        v_flex()
            .flex_1()
            .min_h(px(240.))
            .rounded(px(10.))
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(T.border))
            .overflow_hidden()
            .child(header)
            .child(list)
    }

    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let line = &self.lines[index];
        let problem = self.problem(line);
        let this = cx.entity();
        let mut row = h_flex()
            .id(("test-row", index))
            .w_full()
            .h(px(ROW_H))
            .px(px(12.))
            .gap(px(8.))
            .text_size(px(12.5))
            .border_b_1()
            .border_color(rgb(T.divider))
            .child(
                div()
                    .w(px(W_LINE))
                    .flex_none()
                    .font_family(MONO)
                    .text_color(rgb(T.muted))
                    .child(SharedString::from(line.line.to_string())),
            );
        let ellipsis = |width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
        };
        match (&line.parsed, problem) {
            (_, Some(problem)) => {
                // Skipped: shown in red with the reason, never sent to an engine.
                let id = line
                    .parsed
                    .as_ref()
                    .ok()
                    .and_then(|record| record.id.clone())
                    .unwrap_or_default();
                row = row
                    .bg(rgb(T.danger_bg))
                    .text_color(rgb(T.danger_fg))
                    .child(ellipsis(W_ID).child(SharedString::from(id)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(SharedString::from(format!("Skipped: {problem}")))
                            .id(("test-row-problem", index))
                            .tooltip({
                                let text: SharedString = line.text.clone().into();
                                move |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(text.clone())
                                        .build(window, cx)
                                }
                            }),
                    );
            }
            (Ok(record), None) => {
                row = row
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(T.hover)))
                    .on_click(move |_, window, cx| {
                        this.update(cx, |screen, cx| screen.open_line(index, window, cx))
                    })
                    .child(ellipsis(W_ID).child(SharedString::from(
                        record.id.clone().unwrap_or_else(|| "—".into()),
                    )))
                    .child(
                        ellipsis(W_EXPECT)
                            .font_family(MONO)
                            .child(SharedString::from(record.expectation())),
                    );
                let run = self.run.as_ref();
                let position = run.and_then(|run| run.position_of.get(index).copied().flatten());
                let columns = run.map_or(self.settings.engines.len(), |run| run.engines.len());
                for slot in 0..columns {
                    let cell = run
                        .zip(position)
                        .and_then(|(run, position)| run.cells.get(slot)?.get(position));
                    row = row.child(render_cell(cell));
                }
            }
            (Err(_), None) => unreachable!("a parse error is always a problem"),
        }
        row
    }
}

impl Render for TestPositionsScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let side = v_flex()
            .id("test-settings")
            .w(px(340.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .gap(px(20.))
            .p(px(20.))
            .border_r_1()
            .border_color(rgb(T.border))
            .bg(rgb(T.tour_card))
            .child(self.render_file(cx))
            .child(self.render_form(cx));
        let main = if self.lines.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(ui::i18n::t("Test positions")),
                )
                .child(
                    div()
                        .max_w(px(460.))
                        .text_center()
                        .text_color(rgb(T.fg_2))
                        .child(ui::i18n::t(
                            "Load an EPD suite such as WAC or Bratko-Kopec. Each engine searches every position, and a position counts as solved when it plays a bm move (and no am move) or finds the dm mate.",
                        )),
                )
        } else {
            v_flex()
                .size_full()
                .gap(px(16.))
                .p(px(20.))
                .children(self.run.as_ref().map(|run| self.render_summary(run)))
                .child(self.render_table(cx))
        };
        h_flex()
            .size_full()
            .items_start()
            .bg(rgb(T.tour_bg))
            .child(side)
            .child(div().flex_1().min_w_0().h_full().child(main))
    }
}

fn render_cell(cell: Option<&Cell>) -> Div {
    let base = div()
        .flex_1()
        .min_w(px(140.))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .font_family(MONO);
    match cell {
        None | Some(Cell::Waiting) => base.text_color(rgb(T.faint)).child("—"),
        Some(Cell::Running(search)) => base
            .text_color(rgb(T.muted))
            .child(SharedString::from(format!("… {}", search_text(search)))),
        Some(Cell::Solved(search)) => {
            // The move in green, then where it was found in muted text beside it.
            let found = search
                .found
                .map(|found| format!("d{} {}", found.depth, format_ms(found.time_ms)));
            base.flex()
                .flex_row()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(T.good))
                        .child(SharedString::from(format!(
                            "✓ {}",
                            search.best.clone().unwrap_or_default()
                        ))),
                )
                .children(found.map(|found| {
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(rgb(T.muted))
                        .child(SharedString::from(found))
                }))
        }
        Some(Cell::Failed(search)) => base
            .text_color(rgb(T.q_blunder))
            .child(SharedString::from(format!("✗ {}", search_text(search)))),
        Some(Cell::Error(err)) => base
            .text_color(rgb(T.danger_fg))
            .child(SharedString::from(err.clone())),
    }
}

/// "Qg6 +3.20 d18".
fn search_text(search: &Search) -> String {
    let mut parts = Vec::new();
    if let Some(best) = &search.best {
        parts.push(best.clone());
    }
    if let Some(score) = search.score {
        parts.push(score.display());
    }
    if search.depth > 0 {
        parts.push(format!("d{}", search.depth));
    }
    parts.join(" ")
}

fn meter(fraction: f32, colour: u32) -> Div {
    div()
        .h(px(6.))
        .w_full()
        .rounded(px(3.))
        .bg(rgb(T.tour_tile_border))
        .child(
            div()
                .h_full()
                .rounded(px(3.))
                .bg(rgb(colour))
                .w(relative(fraction.clamp(0., 1.))),
        )
}

/// The limit field's text for the chosen kind.
fn limit_text(settings: &TestPositionSettings) -> String {
    match settings.limit {
        TestLimitKind::Depth => settings.depth.to_string(),
        TestLimitKind::Nodes => settings.nodes.to_string(),
        TestLimitKind::Time => {
            let seconds = settings.time_ms as f64 / 1000.;
            format!("{seconds}")
        }
    }
}

/// "500000", "500k", "2M", "1.5m".
fn parse_count(text: &str) -> Option<u64> {
    let text = text.trim();
    let (number, scale) = match text.chars().last()? {
        'k' | 'K' => (&text[..text.len() - 1], 1_000.),
        'm' | 'M' => (&text[..text.len() - 1], 1_000_000.),
        'g' | 'G' | 'b' | 'B' => (&text[..text.len() - 1], 1_000_000_000.),
        _ => (text, 1.),
    };
    let value = number.trim().parse::<f64>().ok()? * scale;
    (value.is_finite() && value >= 0.).then(|| value.round() as u64)
}

/// "850 ms", "4.2 s", "3m 05s".
fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.)
    } else {
        let secs = ms / 1000;
        if secs < 3600 {
            format!("{}m {:02}s", secs / 60, secs % 60)
        } else {
            format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_accept_suffixes() {
        assert_eq!(parse_count("500000"), Some(500_000));
        assert_eq!(parse_count("500k"), Some(500_000));
        assert_eq!(parse_count("1.5M"), Some(1_500_000));
        assert_eq!(parse_count("abc"), None);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_ms(850), "850 ms");
        assert_eq!(format_ms(4200), "4.2 s");
        assert_eq!(format_ms(185_000), "3m 05s");
    }

    #[test]
    fn time_limits_show_seconds() {
        let settings = TestPositionSettings {
            time_ms: 500,
            ..Default::default()
        };
        assert_eq!(limit_text(&settings), "0.5");
    }
}
