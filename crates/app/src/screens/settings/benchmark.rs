//! Measuring an engine's speed with its bench command or a fixed-time suite.

use super::*;

/// `bench` if the engine supports it, otherwise a fixed-time search on a small suite.
pub(super) fn benchmark(config: &EngineConfig) -> String {
    let output = Command::new(&config.path).arg("bench").output();
    if let Ok(output) = output {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(result) = parse_bench(&text) {
            return result;
        }
    }
    const SUITE: [&str; 3] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bq1rk1/2p1bppp/p1np1n2/1p2p3/4P3/1BP2N1P/PP1P1PP1/RNBQR1K1 b - - 0 9",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ];
    let options = config
        .options
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let handle = EngineHandle::spawn(config.path.clone(), Vec::new(), options);
    let events = handle.events();
    let mut speeds = Vec::new();
    for (index, fen) in SUITE.iter().enumerate() {
        let search_id = index as u64 + 1;
        handle.search(SearchRequest {
            chess960: false,
            search_moves: Vec::new(),
            fen: (*fen).to_owned(),
            moves: Vec::new(),
            limits: Some(Limits {
                movetime_ms: Some(2000),
                ..Default::default()
            }),
            search_id,
            // Unrelated positions: each is its own game.
            game: search_id,
        });
        let mut speed = None;
        loop {
            match smol::block_on(events.recv()) {
                Ok(EngineEvent::Analysis { update, .. }) => speed = update.nps.or(speed),
                Ok(EngineEvent::BestMove {
                    search_id: done, ..
                }) if done == search_id => break,
                Ok(EngineEvent::Crashed(reason)) => return format!("Benchmark failed: {reason}"),
                Ok(_) => {}
                Err(_) => return "Benchmark failed: engine closed".to_owned(),
            }
        }
        speeds.extend(speed);
    }
    handle.quit();
    if speeds.is_empty() {
        return "Benchmark finished, but the engine reported no speed.".to_owned();
    }
    let average = speeds.iter().sum::<u64>() / speeds.len() as u64;
    format!("Fixed-time suite: {} n/s", format_count(average))
}

pub(super) fn parse_bench(text: &str) -> Option<String> {
    let line = text
        .lines()
        .rev()
        .find(|line| line.contains("Nodes/second"))?;
    let value: u64 = line.split(':').nth(1)?.trim().parse().ok()?;
    Some(format!("bench: {} n/s", format_count(value)))
}

impl SettingsScreen {
    pub(super) fn run_benchmark(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let Some(config) = self.model.read(cx).engine(id).cloned() else {
            return;
        };
        if self.model.read(cx).demo {
            self.benchmark = Some(
                ui::i18n::t("Benchmark needs a real engine; demo mode only has scripted lines.")
                    .into(),
            );
            cx.notify();
            return;
        }
        self.benchmark = Some(ui::i18n::t("Running benchmark…").into());
        cx.notify();
        let task = cx.background_spawn(async move { benchmark(&config) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.benchmark = Some(result.into());
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_output_is_parsed() {
        let text =
            "stuff\nTotal time (ms) : 100\nNodes searched  : 1000\nNodes/second    : 123456\n";
        assert_eq!(parse_bench(text).as_deref(), Some("bench: 123,456 n/s"));
        assert_eq!(parse_bench("no numbers"), None);
    }
}
