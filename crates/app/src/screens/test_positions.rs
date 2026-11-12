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
    UniformListScrollHandle, WeakEntity, Window, div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::input::InputState;
use ui::{
    MONO, T,
    widgets::{btn, check, h_flex, meter, section_label, segmented, v_flex},
};

use crate::{
    config::{TestGoal, TestLimitKind, TestPositionSettings},
    controls::{field, new_input, text_input, toggle},
    model::{AppModel, pick_directory, pick_path},
    shell::Workspace,
    test_suite::{self, Cell, RunEvent, RunSettings, Search},
};

use self::form::limit_text;

mod form;
mod results;

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
        // Takes everything already queued, then draws once.
        let reader = crate::event_reader::spawn_batched(rx, None, cx, |screen, batch, cx| {
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
