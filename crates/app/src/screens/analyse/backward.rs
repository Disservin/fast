//! Backward analysis: stepping back through the game while the engines search each position.

use super::*;

/// Each position is shown at least this long, even when a search ends at once.
pub(super) const BACKWARD_DWELL: std::time::Duration = std::time::Duration::from_millis(250);

impl AnalyseScreen {
    /// Starts analysing the shown line from its end back to the first move, or stops.
    pub fn toggle_backward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backward.is_some() {
            self.stop_backward(Some(ui::i18n::t("Backwards analysis stopped.")), cx);
            return;
        }
        if self.model.read(cx).analysis_engines().is_empty() {
            self.model.toast(
                "Add an engine for analysis first, in Settings › Engines.",
                cx,
            );
            return;
        }
        if self.model.read(cx).demo {
            self.model.toast(
                "Backwards analysis needs a real engine; demo mode only has scripted lines.",
                cx,
            );
            return;
        }
        // A fixed node count, set in Settings › Analysis, so every position gets the same
        // search whatever the limit for ordinary analysis.
        let nodes = self.model.read(cx).config.backward_nodes.max(1);
        self.live.update(cx, |live, cx| {
            live.limit_override = Some(crate::config::AnalysisLimit::Nodes(nodes));
            if !live.on {
                live.toggle(cx);
            }
            live.set_paused(false, cx);
        });
        let end = self
            .tree
            .line_through(self.current)
            .last()
            .copied()
            .unwrap_or(self.current);
        // The jump searches the end position under the new limit.
        self.jump(end, window, cx);
        let poll = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let alive = this.update_in(cx, |this, window, cx| this.backward_tick(window, cx));
                if !matches!(alive, Ok(true)) {
                    return;
                }
            }
        });
        self.backward = Some(BackwardRun {
            node: end,
            started: std::time::Instant::now(),
            _poll: poll,
        });
        cx.notify();
    }

    /// Moves one position back once every engine has finished; false ends the poll.
    pub(super) fn backward_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.backward else {
            return false;
        };
        if run.node != self.current {
            self.stop_backward(None, cx);
            return false;
        }
        if run.started.elapsed() < BACKWARD_DWELL {
            return true;
        }
        let (loaded, idle) = {
            let live = self.live.read(cx);
            (live.loaded(), live.idle())
        };
        if loaded == 0 {
            self.stop_backward(
                Some(ui::i18n::t(
                    "Backwards analysis stopped: no engine is running.",
                )),
                cx,
            );
            return false;
        }
        if !idle {
            return true;
        }
        let Some(parent) = self.tree.node(self.current).parent else {
            self.stop_backward(Some(ui::i18n::t("Backwards analysis finished.")), cx);
            return false;
        };
        // Recorded before the jump, so the run sees its own move and carries on.
        if let Some(run) = &mut self.backward {
            run.node = parent;
            run.started = std::time::Instant::now();
        }
        self.jump(parent, window, cx);
        true
    }

    pub(super) fn stop_backward(&mut self, message: Option<&str>, cx: &mut Context<Self>) {
        // Dropping the run drops its poll task.
        if self.backward.take().is_none() {
            return;
        }
        self.live.update(cx, |live, cx| {
            live.limit_override = None;
            live.search(cx);
        });
        if let Some(message) = message {
            self.model.toast(message.to_owned(), cx);
        }
        cx.notify();
    }
}
