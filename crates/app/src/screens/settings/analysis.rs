//! Settings › Analysis: backward analysis and the kibitz engine.

use super::*;

/// Node presets for backwards analysis.
pub(super) const BACKWARD_PRESETS: [(u64, &str); 5] = [
    (100_000, "100k"),
    (250_000, "250k"),
    (500_000, "500k"),
    (1_000_000, "1M"),
    (5_000_000, "5M"),
];

/// A node count as typed: digits, optionally with separators and a k or M suffix.
pub(super) fn parse_nodes(text: &str) -> Option<u64> {
    let text: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | '_' | ' ' | '\''))
        .collect();
    let (digits, scale) = match text.chars().last()? {
        'k' | 'K' => (&text[..text.len() - 1], 1_000.),
        'm' | 'M' => (&text[..text.len() - 1], 1_000_000.),
        _ => (text.as_str(), 1.),
    };
    let value: f64 = digits.parse().ok()?;
    let nodes = (value * scale).round();
    (value.is_finite() && (1. ..=1e12).contains(&nodes)).then_some(nodes as u64)
}

impl SettingsScreen {
    pub(super) fn set_backward_nodes(
        &mut self,
        nodes: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.backward_error = None;
        self.backward_nodes.update(cx, |state, cx| {
            state.set_value(nodes.to_string(), window, cx)
        });
        self.model.update(cx, |model, cx| {
            if model.config.backward_nodes != nodes {
                model.config.backward_nodes = nodes;
                model.save_config(cx);
            }
        });
        cx.notify();
    }

    /// Changes the kibitz settings and saves them; the next preview uses them.
    pub(super) fn update_kibitz(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::config::KibitzConfig),
    ) {
        self.model.update(cx, |model, cx| {
            change(&mut model.config.kibitz);
            model.save_config(cx);
        });
        cx.notify();
    }

    /// The kibitz engine: whether it runs, which engine, and its threads and hash.
    pub(super) fn render_kibitz(&mut self, cx: &mut Context<Self>) -> Div {
        let model = self.model.read(cx);
        let kibitz = model.config.kibitz.clone();
        let engines: Vec<(EngineId, String)> = model
            .config
            .engines
            .iter()
            .map(|engine| (engine.id, engine.name.clone()))
            .collect();
        let this = cx.entity();
        let toggle_this = this.clone();
        let toggle = check("kibitz-on", kibitz.enabled, "Analyse with a kibitz engine").on_change(
            move |value, _, cx| {
                toggle_this.update(cx, |screen, cx| {
                    screen.update_kibitz(cx, |kibitz| kibitz.enabled = value)
                })
            },
        );
        let mut choices = vec![(None, ui::i18n::t("First analysis engine").into(), false)];
        choices.extend(
            engines
                .into_iter()
                .map(|(id, name)| (Some(id), SharedString::from(name), false)),
        );
        let chosen = kibitz
            .engine
            .filter(|id| choices.iter().any(|(choice, ..)| *choice == Some(*id)));
        let engine_this = this.clone();
        let engine_choice = segmented("kibitz-engine", choices, chosen, move |value, _, cx| {
            engine_this.update(cx, |screen, cx| {
                screen.update_kibitz(cx, |kibitz| kibitz.engine = value)
            })
        })
        .flex_wrap()
        .gap_y(px(6.));
        let cores = std::thread::available_parallelism()
            .map_or(64, |cores| cores.get() as u32)
            .max(1);
        let stepper = |id: &'static str,
                       value: String,
                       down: Option<u32>,
                       up: Option<u32>,
                       set: fn(&mut crate::config::KibitzConfig, u32)| {
            let (down_this, up_this) = (this.clone(), this.clone());
            let apply = move |this: &Entity<Self>, value: Option<u32>, cx: &mut gpui::App| {
                if let Some(value) = value {
                    this.update(cx, |screen, cx| {
                        screen.update_kibitz(cx, |kibitz| set(kibitz, value))
                    });
                }
            };
            ui::widgets::stepper(id, value)
                .mini()
                .value_width(64.)
                .on_down(down.is_some(), move |_, cx| apply(&down_this, down, cx))
                .on_up(up.is_some(), move |_, cx| apply(&up_this, up, cx))
        };
        let threads = kibitz.threads.max(1);
        let hash = kibitz.hash_mb.max(1);
        section(ui::i18n::t("Kibitz engine"))
            .child(option_row(
                ui::i18n::t("Line previews"),
                Some(ui::i18n::t(
                    "Clicking a move in Line preview analyses that position with a second \
                     engine, shown below the line, while the main analysis keeps searching \
                     the game position. Off: the main engines follow the preview.",
                )),
                toggle,
            ))
            .child(option_row(ui::i18n::t("Engine"), None, engine_choice))
            .child(option_row(
                ui::i18n::t("Threads"),
                None,
                stepper(
                    "kibitz-threads",
                    threads.to_string(),
                    (threads > 1).then(|| threads - 1),
                    (threads < cores).then(|| threads + 1),
                    |kibitz, value| kibitz.threads = value,
                ),
            ))
            .child(option_row(
                "Hash",
                None,
                stepper(
                    "kibitz-hash",
                    engine::format_hash(i64::from(hash)),
                    (hash > 1).then_some(hash / 2),
                    (hash < 65_536).then(|| hash.saturating_mul(2).min(65_536)),
                    |kibitz, value| kibitz.hash_mb = value,
                ),
            ))
    }

    pub(super) fn render_analysis(&mut self, cx: &mut Context<Self>) -> Div {
        let current = self.model.read(cx).config.backward_nodes;
        let mut presets = h_flex().gap(px(6.)).flex_wrap();
        for (index, (nodes, label)) in BACKWARD_PRESETS.into_iter().enumerate() {
            let this = cx.entity();
            presets = presets.child(
                btn(("backward-preset", index), label)
                    .h(30.)
                    .pad(10.)
                    .selected(current == nodes)
                    .on_click(this.listen(move |screen, window, cx| {
                        screen.set_backward_nodes(nodes, window, cx)
                    })),
            );
        }
        let field = v_flex()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(div().w(px(180.)).child(text_input(&self.backward_nodes)))
                    .child(
                        div()
                            .text_as(13., T.muted)
                            .child(format!("{} per position", engine::format_nodes(current))),
                    ),
            )
            .child(presets)
            .children(
                self.backward_error
                    .clone()
                    .map(|error| div().text_as(12., T.warn).child(error)),
            );
        v_flex()
            .gap(px(16.))
            .child(page_title(ui::i18n::t("Analysis")))
            .child(self.render_kibitz(cx))
            .child(
                section(ui::i18n::t("Backwards analysis"))
                    .child(option_row(
                        ui::i18n::t("Nodes per position"),
                        Some(ui::i18n::t(
                            "Each analysis engine searches this many nodes in every position, \
                             from the last move back to the first. A fixed node count gives \
                             every position the same effort on any machine.",
                        )),
                        field,
                    ))
                    .child(div().pt(px(10.)).text_as(12., T.muted).child(ui::i18n::t(
                        "Start it with Analyse backwards above the game graph, or its \
                                 shortcut. The search limit for ordinary analysis is not used.",
                    ))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_counts_are_read_with_suffixes_and_separators() {
        assert_eq!(parse_nodes("500000"), Some(500_000));
        assert_eq!(parse_nodes(" 500k "), Some(500_000));
        assert_eq!(parse_nodes("1.5M"), Some(1_500_000));
        assert_eq!(parse_nodes("2_000_000"), Some(2_000_000));
        assert_eq!(parse_nodes("1,000,000"), Some(1_000_000));
        assert_eq!(parse_nodes("0"), None);
        assert_eq!(parse_nodes("-5k"), None);
        assert_eq!(parse_nodes("lots"), None);
        assert_eq!(parse_nodes(""), None);
    }
}
