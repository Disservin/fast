//! The test suite form: the file of positions, the engines and the search limits.

use super::*;

/// The limit field's text for the chosen kind.
pub(super) fn limit_text(settings: &TestPositionSettings) -> String {
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
pub(super) fn parse_count(text: &str) -> Option<u64> {
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

impl TestPositionsScreen {
    pub(super) fn choose_file(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn choose_syzygy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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

    /// Reads the text fields into the settings, or says what is wrong.
    pub(super) fn read_form(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
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

    pub(super) fn render_file(&self, cx: &mut Context<Self>) -> Div {
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
                            .on_click(choose.listen(move |screen, _, cx| screen.choose_file(cx))),
                    ),
            )
            .children(self.load_error.clone().map(|err| {
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(T.danger_fg))
                    .child(err)
            }))
    }

    pub(super) fn render_engines(&self, cx: &mut Context<Self>) -> Div {
        let engines = self.model.read(cx).config.engines.clone();
        let mut list = v_flex().gap(px(6.));
        if engines.is_empty() {
            list = list.child(div().text_as(13., T.muted).child(ui::i18n::t(
                "No engines yet. Add them in Settings › Engines.",
            )));
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
                .on_change(this.listen_with(move |screen, on, _, cx| {
                    screen.settings.engines.retain(|e| *e != id);
                    if on {
                        screen.settings.engines.push(id);
                    }
                    cx.notify();
                })),
            );
        }
        v_flex()
            .gap(px(8.))
            .child(section_label("Engines"))
            .child(list)
    }

    pub(super) fn render_form(&self, cx: &mut Context<Self>) -> Div {
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
                .text_as(12., T.muted)
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
            .child(toggle(
                "test-clear-hash",
                self.settings.clear_hash,
                "Clear hash between positions",
                cx,
                |screen, on| screen.settings.clear_hash = on,
            ))
            .child(section_label("Running"))
            .child(toggle(
                "test-parallel",
                self.settings.parallel,
                "Run in parallel",
                cx,
                |screen, on| screen.settings.parallel = on,
            ))
            .child(
                div()
                    .text_as(12., T.muted)
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
            .child(toggle(
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
                    .on_click(stop.listen(move |screen, _, cx| screen.stop(cx)))
            } else {
                btn("test-start", ui::i18n::t("Run test"))
                    .primary()
                    .h(38.)
                    .full()
                    .disabled(self.lines.is_empty())
                    .on_click(start.listen(move |screen, _, cx| screen.start(cx)))
            })
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
    fn time_limits_show_seconds() {
        let settings = TestPositionSettings {
            time_ms: 500,
            ..Default::default()
        };
        assert_eq!(limit_text(&settings), "0.5");
    }
}
