//! Leaks: the positions where games most often left the repertoire, drilled on their own.

use super::*;

/// The leaks the Leaks panel lists, and the ones Drill these lines practises: the
/// first two seen in at least two games.
pub(super) fn shown_leaks(leaks: &[trainer::Leak]) -> impl Iterator<Item = &trainer::Leak> {
    const MIN_LEAK_GAMES: u32 = 2;
    leaks
        .iter()
        .filter(|leak| leak.games >= MIN_LEAK_GAMES)
        .take(2)
}

impl TrainScreen {
    /// Puts the leaks the Leaks panel lists, where the repertoire has a prepared
    /// answer, at the front of the session: each asks for the prepared move in the
    /// position where a game left the repertoire. The rest of the session stays queued
    /// behind them, without repeating those positions.
    pub(super) fn drill_leaks(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let drills: Vec<Drill> = shown_leaks(&model.leaks)
            .filter_map(|leak| {
                Some(Drill {
                    path: leak.path.clone(),
                    answer: leak.prepared.clone()?,
                    note: format!(
                        "You played {} here in {} games. The repertoire move is {}.",
                        leak.played,
                        leak.games,
                        leak.prepared.as_deref().unwrap_or_default()
                    ),
                })
            })
            .collect();
        if drills.is_empty() {
            self.message = Some(ui::i18n::t("No leaks to drill.").into());
            cx.notify();
            return;
        }
        let key = |drill: &Drill| {
            (
                trainer::Repertoire::position_key(&drill.path),
                drill.answer.clone(),
            )
        };
        let already_next = self.queue.len() >= drills.len()
            && drills
                .iter()
                .zip(&self.queue)
                .all(|(drill, queued)| key(drill) == key(queued));
        let count = drills.len();
        let added: std::collections::HashSet<_> = drills.iter().map(key).collect();
        let before = self.queue.len();
        self.queue.retain(|drill| !added.contains(&key(drill)));
        let replaced = before - self.queue.len();
        self.total = (self.total + drills.len()).saturating_sub(replaced);
        self.queue.splice(0..0, drills);
        self.start_drill(cx);
        self.persist_session(cx);
        // Without a word the click can look like it did nothing, since the queue may
        // already start with these positions.
        let text = match (already_next, count) {
            (true, 1) => {
                ui::i18n::t("This leak is already the next position in the session.").to_owned()
            }
            (true, _) => {
                format!("These {count} leaks are already the next positions in the session.")
            }
            (false, 1) => ui::i18n::t("The leak is now the next position to drill.").to_owned(),
            (false, _) => format!("The {count} leaks are now the next positions to drill."),
        };
        self.model.toast(text, cx);
    }

    pub(super) fn render_leaks(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        const MIN_GAP_GAMES: u32 = 5;
        let this = cx.entity();
        let model = self.model.read(cx);
        let row = |title: String, subtitle: String, games: u32, warn: bool| {
            v_flex()
                .px(px(16.))
                .py(px(10.))
                .gap(px(2.))
                .border_t_1()
                .border_color(rgb(T.border))
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            mono(format!("{games} games"))
                                .text_size(px(14.))
                                .text_color(rgb(if warn { T.warn } else { T.muted })),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(subtitle),
                )
        };
        let mut body = v_flex();
        let mut shown = 0;
        for leak in shown_leaks(&model.leaks) {
            let number = leak.path.len() as u32;
            let played = chess_core::notation::numbered(number + 1, &leak.played);
            let title = match &leak.prepared {
                Some(prepared) => format!(
                    "{played}{} for {}",
                    if leak.prepared_score.is_some() {
                        "?!"
                    } else {
                        ""
                    },
                    chess_core::notation::numbered(number + 1, prepared)
                ),
                None => played,
            };
            let subtitle = match leak.prepared_score {
                Some(prepared) => format!(
                    "Score {:.0}% vs {:.0}% with the prepared move",
                    leak.score * 100.0,
                    prepared * 100.0
                ),
                None => {
                    let start = leak.path.len().saturating_sub(2);
                    format!("After {}", format_pv(start as u32, &leak.path[start..]))
                }
            };
            body = body.child(row(title, subtitle, leak.games, true));
            shown += 1;
        }
        for gap in model
            .gaps
            .iter()
            .filter(|gap| gap.games >= MIN_GAP_GAMES)
            .take(1)
        {
            let start = gap.path.len().saturating_sub(3);
            let title = format_pv(start as u32, &gap.path[start..]);
            body = body.child(row(
                title,
                ui::i18n::t("Not in repertoire yet").to_owned(),
                gap.games,
                false,
            ));
            shown += 1;
        }
        if shown == 0 {
            body = body.child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(
                        "No leaks found. Import your games to see where you leave the repertoire.",
                    ),
            );
        }
        panel()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .px(px(16.))
                    .py(px(12.))
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(ui::i18n::t("Leaks from your games")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(T.muted))
                            .child(ui::i18n::t("Where you left the repertoire")),
                    ),
            )
            .child(body)
            .child(
                h_flex()
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(
                        btn("drill-leaks", "Drill these lines")
                            .primary()
                            .tooltip(ui::i18n::t(
                                "Practise the repertoire move in each position above, next in this session",
                            ))
                            .disabled(
                                !shown_leaks(&model.leaks).any(|leak| leak.prepared.is_some()),
                            )
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| screen.drill_leaks(cx));
                            }),
                    ),
            )
    }
}
