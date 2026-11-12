//! Editable live-analysis limits, kept in sync with the header's Infinite toggle.

use gpui::{Context, Entity, IntoElement, Render, Subscription, Window, div, prelude::*, px};
use gpui_component::{
    input::{InputEvent, InputState},
    select::SelectEvent,
};
use ui::widgets::h_flex;

use crate::{
    config::AnalysisLimit,
    controls::{TextSelect, new_input, new_select, select_el, text_input},
    live::LiveAnalysis,
    model::AppModel,
};

/// Lets the user choose a search type and enter any positive value for it.
pub(in crate::screens::analyse) struct SearchLimitControls {
    kind: TextSelect,
    value: Entity<InputState>,
    live: Entity<LiveAnalysis>,
    limit: AnalysisLimit,
    values: [String; 3],
    _subscriptions: Vec<Subscription>,
}

/// Splits a stored limit into its selector row and editable number.
fn limit_parts(limit: AnalysisLimit) -> (usize, String) {
    match limit {
        AnalysisLimit::Seconds(value) => (0, value.to_string()),
        AnalysisLimit::Depth(value) => (1, value.to_string()),
        AnalysisLimit::Nodes(value) => (2, value.to_string()),
        AnalysisLimit::Infinite => (3, String::new()),
    }
}

impl SearchLimitControls {
    /// Restores the saved limit and follows changes made through the header toggle.
    pub(in crate::screens::analyse) fn new(
        model: Entity<AppModel>,
        live: Entity<LiveAnalysis>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = &model.read(cx).config;
        let limit = config.analysis_limit;
        let mut values = ["30".into(), "20".into(), "500000".into()];
        let (finite_kind, finite_value) = limit_parts(config.finite_limit);
        if finite_kind < 3 {
            values[finite_kind] = finite_value;
        }
        let (row, number) = limit_parts(limit);
        if row < 3 {
            values[row] = number.clone();
        }
        let kind = new_select(
            ["Time", "Depth", "Nodes", "Infinite"]
                .map(Into::into)
                .to_vec(),
            row,
            window,
            cx,
        );
        let value = new_input(number, "Value", window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&kind, window, |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(_)) = event {
                    let row = this
                        .kind
                        .read(cx)
                        .selected_index(cx)
                        .map(|index| index.row)
                        .unwrap_or(3);
                    if row < 3 {
                        this.value.update(cx, |state, cx| {
                            state.set_value(this.values[row].clone(), window, cx)
                        });
                    }
                    this.apply(cx);
                }
            }),
            cx.subscribe(&value, |this, _, event, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.apply(cx);
                }
                cx.notify();
            }),
            cx.observe_in(&model, window, |this, model, window, cx| {
                let limit = model.read(cx).config.analysis_limit;
                if limit == this.limit {
                    return;
                }
                this.limit = limit;
                let (row, number) = limit_parts(limit);
                this.kind.update(cx, |state, cx| {
                    state.set_selected_index(Some(gpui_component::IndexPath::new(row)), window, cx)
                });
                if row < 3 {
                    this.values[row] = number.clone();
                    this.value
                        .update(cx, |state, cx| state.set_value(number, window, cx));
                }
                cx.notify();
            }),
        ];
        Self {
            kind,
            value,
            live,
            limit,
            values,
            _subscriptions: subscriptions,
        }
    }

    /// Reads a positive value in the selected unit without truncating large numbers.
    fn entered_limit(&self, cx: &gpui::App) -> Option<AnalysisLimit> {
        let row = self.kind.read(cx).selected_index(cx)?.row;
        if row == 3 {
            return Some(AnalysisLimit::Infinite);
        }
        let number = self
            .value
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)?;
        match row {
            0 => Some(AnalysisLimit::Seconds(u32::try_from(number).ok()?)),
            1 => Some(AnalysisLimit::Depth(u32::try_from(number).ok()?)),
            2 => Some(AnalysisLimit::Nodes(number)),
            _ => None,
        }
    }

    /// Saves a valid limit and restarts the current search once the edit is finished.
    fn apply(&mut self, cx: &mut Context<Self>) {
        if let Some(limit) = self.entered_limit(cx) {
            let (row, number) = limit_parts(limit);
            if row < 3 {
                self.values[row] = number;
            }
            if limit != self.limit {
                self.limit = limit;
                self.live.update(cx, |live, cx| live.set_limit(limit, cx));
            }
        }
        cx.notify();
    }
}

impl Render for SearchLimitControls {
    /// Shows the type beside its numeric value, hiding the value for Infinite.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = self
            .kind
            .read(cx)
            .selected_index(cx)
            .map(|index| index.row)
            .unwrap_or(3);
        h_flex()
            .gap(px(6.))
            .child(div().w(px(116.)).child(select_el(&self.kind)))
            .when(row < 3, |controls| {
                controls.child(
                    div().w(px(132.)).child(
                        text_input(&self.value)
                            .mono()
                            .invalid(self.entered_limit(cx).is_none())
                            .suffix((row == 0).then(|| ui::i18n::t("s").to_owned())),
                    ),
                )
            })
    }
}
