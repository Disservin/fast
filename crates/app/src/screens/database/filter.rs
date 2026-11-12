//! Filtering and sorting the game list, saved searches and player suggestions.

use super::*;

/// Splits "B90–B99" into its bounds.
pub(super) fn parse_eco(text: &str) -> (String, String) {
    let text = text.trim().to_uppercase();
    match text.split_once(['–', '-']) {
        Some((low, high)) => (low.trim().to_owned(), high.trim().to_owned()),
        None => (text.clone(), text),
    }
}

impl DatabaseScreen {
    pub(super) fn build_filter(&self, cx: &gpui::App) -> Filter {
        let (eco_min, eco_max) = parse_eco(self.eco.read(cx).value().as_ref());
        let colour = self
            .colour
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let time = self
            .time_control
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let player = self.player.read(cx).value().to_string();
        // Typing any of the user's names finds the games under all of them.
        let my_names = self.model.read(cx).config.my_names();
        let player_aliases = if my_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(player.trim()))
        {
            my_names
        } else {
            Vec::new()
        };
        let pattern = GameTree::from_fen(self.pattern_fen.read(cx).value().trim())
            .ok()
            .map(|tree| chess_core::PositionPattern::of(&tree.node(GameTree::ROOT).pos));
        Filter {
            pawn_structure: self
                .pattern_pawns
                .then_some(pattern)
                .flatten()
                .map(|p| p.pawn_hash),
            material: self
                .pattern_material
                .then_some(pattern)
                .flatten()
                .map(|p| p.material),
            player,
            player_aliases,
            side: match colour {
                1 => SideFilter::White,
                2 => SideFilter::Black,
                _ => SideFilter::Any,
            },
            eco_min,
            eco_max,
            min_opponent_rating: self.opp_rating.read(cx).value().trim().parse().ok(),
            time_classes: match time {
                1 => vec![TimeClass::Rapid, TimeClass::Classical],
                2 => vec![TimeClass::Classical],
                3 => vec![TimeClass::Rapid],
                4 => vec![TimeClass::Blitz],
                _ => Vec::new(),
            },
            position: self
                .reached_position
                .as_ref()
                .filter(|_| self.reached)
                .map(|(hash, _)| *hash),
            only_left_repertoire: self.only_left_rep,
            only_annotated: self.only_annotated,
            date_from: date_bound(self.date_from.read(cx).value().as_ref(), false),
            date_to: date_bound(self.date_to.read(cx).value().as_ref(), true),
            event: self.event.read(cx).value().trim().to_owned(),
            result: match self
                .result
                .read(cx)
                .selected_index(cx)
                .map(|index| index.row)
            {
                Some(1) => Some(ResultFilter::Win),
                Some(2) => Some(ResultFilter::Draw),
                Some(3) => Some(ResultFilter::Loss),
                _ => None,
            },
            min_rating: number(&self.min_rating, cx),
            max_rating: number(&self.max_rating, cx),
            min_moves: number(&self.min_moves, cx),
            max_moves: number(&self.max_moves, cx),
            sort: self.sort,
        }
    }

    pub(super) fn summary_for(&self, filter: &Filter, cx: &gpui::App) -> SharedString {
        let mut parts = Vec::new();
        if filter.pawn_structure.is_some() {
            parts.push(ui::i18n::t("Same pawn structure").to_owned());
        }
        if filter.material.is_some() {
            parts.push(ui::i18n::t("Same material").to_owned());
        }
        if let Some((_, label)) = self.reached_position.as_ref().filter(|_| self.reached) {
            parts.push(label.clone());
        } else if !filter.eco_min.is_empty() {
            parts.push(format!("ECO {}–{}", filter.eco_min, filter.eco_max));
        }
        match filter.side {
            SideFilter::White => parts.push("as White".to_owned()),
            SideFilter::Black => parts.push("as Black".to_owned()),
            SideFilter::Any => {
                if !filter.player.trim().is_empty() {
                    parts.push(filter.player.trim().to_owned());
                }
            }
        }
        if let Some(result) = filter.result {
            let player = !filter.player.trim().is_empty();
            parts.push(
                match (result, player) {
                    (ResultFilter::Win, true) => "won",
                    (ResultFilter::Loss, true) => "lost",
                    (ResultFilter::Draw, _) => "drawn",
                    (ResultFilter::Win, false) => "1–0",
                    (ResultFilter::Loss, false) => "0–1",
                }
                .to_owned(),
            );
        }
        let typed = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        match (typed(&self.date_from), typed(&self.date_to)) {
            (from, to) if from.is_empty() && to.is_empty() => {}
            (from, to) if to.is_empty() => parts.push(format!("from {from}")),
            (from, to) if from.is_empty() => parts.push(format!("until {to}")),
            (from, to) => parts.push(format!("{from}–{to}")),
        }
        if !filter.event.is_empty() {
            parts.push(format!("“{}”", filter.event));
        }
        if filter.only_left_repertoire {
            parts.push("left repertoire".to_owned());
        }
        if filter.only_annotated {
            parts.push("annotated".to_owned());
        }
        parts.join(", ").into()
    }

    /// The sort saved for the selected database: its own, else file order for a
    /// database imported from a file, else newest first.
    pub(super) fn saved_sort(&self, cx: &gpui::App) -> Sort {
        let Some(entry) = &self.db_snapshot else {
            return Sort::default();
        };
        let key = entry.path.to_string_lossy().into_owned();
        let saved = self
            .model
            .read(cx)
            .config
            .database_sorts
            .get(&key)
            .and_then(|saved| {
                Some(Sort {
                    key: SortKey::from_name(&saved.key)?,
                    descending: saved.descending,
                })
            });
        saved.unwrap_or(if entry.source_file.is_some() {
            Sort::FILE
        } else {
            Sort::default()
        })
    }

    /// Sorts the table by `key`, reversing it when it is already sorted by that key,
    /// and remembers the choice for this database.
    pub(super) fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
        let descending = if self.sort.key == key {
            !self.sort.descending
        } else {
            // Names read A to Z; numbers, dates and accuracy biggest or newest first.
            matches!(key, SortKey::Moves | SortKey::Date | SortKey::Accuracy)
        };
        if let Some(entry) = &self.db_snapshot {
            let path = entry.path.to_string_lossy().into_owned();
            self.model.edit_config(cx, |config| {
                config.database_sorts.insert(
                    path,
                    crate::config::SavedSort {
                        key: key.as_str().to_owned(),
                        descending,
                    },
                );
            });
        }
        self.apply_filter(cx);
    }

    /// Runs the filter once typing has paused for a moment. Each change restarts
    /// the wait, and the rows on screen stay until the new ones arrive.
    pub(super) fn schedule_filter(&mut self, cx: &mut Context<Self>) {
        self.filter_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FILTER_DELAY).await;
            let _ = this.update(cx, |this, cx| this.run_query(cx));
        }));
        cx.notify();
    }

    /// Words the Result choices for a player (Win, Draw, Loss) when one is typed, and
    /// as board results (1–0, ½–½, 0–1) otherwise, keeping the choice.
    pub(super) fn sync_result_labels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let for_player = !self.player.read(cx).value().trim().is_empty();
        if for_player == self.result_for_player {
            return;
        }
        self.result_for_player = for_player;
        let labels = if for_player {
            PLAYER_RESULTS
        } else {
            BOARD_RESULTS
        };
        let items: Vec<SharedString> = labels
            .iter()
            .map(|text| SharedString::from(ui::i18n::t(text)))
            .collect();
        self.result.update(cx, |select, cx| {
            let chosen = select.selected_index(cx);
            select.set_items(
                gpui_component::select::SearchableVec::new(items),
                window,
                cx,
            );
            select.set_selected_index(chosen, window, cx);
        });
    }

    /// The filter fields hidden under "More filters" that are set, for its badge.
    pub(super) fn hidden_filters_set(&self, cx: &gpui::App) -> usize {
        let typed = |input: &Entity<InputState>| !input.read(cx).value().trim().is_empty();
        let time = self
            .time_control
            .read(cx)
            .selected_index(cx)
            .is_some_and(|index| index.row != 0);
        [
            typed(&self.eco),
            time,
            typed(&self.opp_rating),
            typed(&self.min_rating) || typed(&self.max_rating),
            typed(&self.min_moves) || typed(&self.max_moves),
            typed(&self.event),
            self.only_annotated,
            self.pattern_pawns || self.pattern_material,
            self.only_left_rep,
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
    }

    /// Counts active filter groups even when their sidebar is folded away.
    pub(super) fn active_filter_count(&self, cx: &gpui::App) -> usize {
        let typed = |input: &Entity<InputState>| !input.read(cx).value().trim().is_empty();
        let chosen = |select: &TextSelect| {
            select
                .read(cx)
                .selected_index(cx)
                .is_some_and(|index| index.row != 0)
        };
        self.hidden_filters_set(cx)
            + [
                typed(&self.player),
                typed(&self.date_from) || typed(&self.date_to),
                chosen(&self.colour),
                chosen(&self.result),
                self.reached,
            ]
            .into_iter()
            .filter(|set| *set)
            .count()
    }

    pub(super) fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_filter_fields(window, cx);
        self.sync_result_labels(window, cx);
        self.apply_filter(cx);
    }

    /// The current filter fields under a name, for the saved searches list.
    pub(super) fn current_search(
        &self,
        name: String,
        cx: &gpui::App,
    ) -> crate::config::SavedFilter {
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let index = |select: &TextSelect| {
            select
                .read(cx)
                .selected_index(cx)
                .map_or(0, |index| index.row)
        };
        crate::config::SavedFilter {
            name,
            player: text(&self.player),
            colour: index(&self.colour),
            result: index(&self.result),
            date_from: text(&self.date_from),
            date_to: text(&self.date_to),
            eco: text(&self.eco),
            time_control: index(&self.time_control),
            opponent_rating: text(&self.opp_rating),
            min_rating: text(&self.min_rating),
            max_rating: text(&self.max_rating),
            min_moves: text(&self.min_moves),
            max_moves: text(&self.max_moves),
            event: text(&self.event),
            only_annotated: self.only_annotated,
            pattern_fen: text(&self.pattern_fen),
            pattern_pawns: self.pattern_pawns,
            pattern_material: self.pattern_material,
            only_left_repertoire: self.only_left_rep,
        }
    }

    /// Fills the filter fields from a saved search and shows its games.
    pub(super) fn apply_saved_search(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(saved) = self.model.read(cx).config.saved_filters.get(index).cloned() else {
            return;
        };
        self.clear_filter_fields(window, cx);
        for (input, value) in [
            (&self.player, &saved.player),
            (&self.date_from, &saved.date_from),
            (&self.date_to, &saved.date_to),
            (&self.eco, &saved.eco),
            (&self.opp_rating, &saved.opponent_rating),
            (&self.min_rating, &saved.min_rating),
            (&self.max_rating, &saved.max_rating),
            (&self.min_moves, &saved.min_moves),
            (&self.max_moves, &saved.max_moves),
            (&self.event, &saved.event),
        ] {
            let value = value.clone();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.player_picked = Some(saved.player.clone());
        self.sync_result_labels(window, cx);
        for (select, row) in [
            (&self.colour, saved.colour),
            (&self.result, saved.result),
            (&self.time_control, saved.time_control),
        ] {
            select.update(cx, |select, cx| {
                select.set_selected_index(Some(gpui_component::IndexPath::new(row)), window, cx)
            });
        }
        self.only_annotated = saved.only_annotated;
        self.pattern_fen.update(cx, |input, cx| {
            input.set_value(saved.pattern_fen, window, cx)
        });
        self.pattern_pawns = saved.pattern_pawns;
        self.pattern_material = saved.pattern_material;
        self.only_left_rep = saved.only_left_repertoire;
        self.more_filters |= self.hidden_filters_set(cx) > 0;
        self.apply_filter(cx);
    }

    /// Opens a field under the Filter heading to name the current search.
    pub(super) fn start_saving_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = new_input(self.summary.to_string(), "Name of the search", window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        self._saving_sub = Some(
            cx.subscribe_in(&input, window, |this, input, event, _, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().trim().to_owned();
                    if !name.is_empty() {
                        let search = this.current_search(name, cx);
                        this.model.update(cx, |model, cx| {
                            model
                                .config
                                .saved_filters
                                .retain(|saved| saved.name != search.name);
                            model.config.saved_filters.push(search);
                            model.save_config(cx);
                        });
                    }
                    this.saving_search = None;
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.saving_search = None;
                    cx.notify();
                }
                _ => {}
            }),
        );
        self.saving_search = Some(input);
        cx.notify();
    }

    pub fn apply_filter(&mut self, cx: &mut Context<Self>) {
        self.close_suggestions(cx);
        self.run_query(cx);
    }

    /// Empties every filter field and option, without running the query.
    pub(super) fn clear_filter_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.player,
            &self.eco,
            &self.opp_rating,
            &self.date_from,
            &self.date_to,
            &self.event,
            &self.min_rating,
            &self.max_rating,
            &self.min_moves,
            &self.max_moves,
        ] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.only_annotated = false;
        self.pattern_pawns = false;
        self.pattern_material = false;
        self.pattern_fen
            .update(cx, |input, cx| input.set_value("", window, cx));
        for select in [&self.colour, &self.time_control, &self.result] {
            select.update(cx, |select, cx| {
                select.set_selected_index(Some(gpui_component::IndexPath::new(0)), window, cx)
            });
        }
        self.player_picked = Some(String::new());
        self.reached = false;
        self.only_left_rep = false;
        self.close_suggestions(cx);
    }

    pub fn search_player(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = query.to_owned();
        self.player_picked = Some(text.trim().to_owned());
        self.player
            .update(cx, |player, cx| player.set_value(text, window, cx));
        self.apply_filter(cx);
    }

    /// Looks up player names in the selected database that match what is typed in
    /// Player, replacing any lookup still running.
    pub(super) fn suggest_players(&mut self, cx: &mut Context<Self>) {
        let text = self.player.read(cx).value().trim().to_owned();
        let picked = self.player_picked.take();
        if text.is_empty() || picked.as_deref() == Some(text.as_str()) {
            return self.close_suggestions(cx);
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let task = cx.background_spawn(async move {
            Database::open(path).and_then(|db| db.player_names(&text, 8))
        });
        self.suggest_task = Some(cx.spawn(async move |this, cx| {
            let names = task.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.player_suggestions = names;
                this.player_highlight = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn close_suggestions(&mut self, cx: &mut Context<Self>) {
        self.suggest_task = None;
        if !self.player_suggestions.is_empty() {
            self.player_suggestions.clear();
            self.player_highlight = None;
            cx.notify();
        }
    }

    /// Moves the highlighted suggestion up or down, wrapping at the ends.
    pub(super) fn step_suggestion(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.player_suggestions.len();
        if count == 0 {
            return;
        }
        self.player_highlight = Some(match (self.player_highlight, down) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
        });
        cx.notify();
    }

    /// Fills Player with a suggested name and shows that player's games.
    pub(super) fn pick_player(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((name, _)) = self.player_suggestions.get(index).cloned() else {
            return;
        };
        self.player_picked = Some(name.clone());
        self.player
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.close_suggestions(cx);
        self.apply_filter(cx);
    }

    /// The suggestions under the Player field, the highlighted one in `selected`.
    pub(super) fn render_player_suggestions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.player_suggestions.is_empty() {
            return None;
        }
        let mut list = v_flex()
            .id("player-suggestions")
            .w(px(245.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(rgb(T.surface))
            .border_as(T.border_strong);
        for (index, (name, games)) in self.player_suggestions.iter().enumerate() {
            let this = cx.entity();
            let highlighted = self.player_highlight == Some(index);
            list = list.child(
                h_flex()
                    .id(("player-suggestion", index))
                    .h(px(28.))
                    .px(px(10.))
                    .gap(px(8.))
                    .justify_between()
                    .cursor_pointer()
                    .text_size(px(13.))
                    .when(highlighted, |row| row.bg(rgb(T.selected)))
                    .when(!highlighted, |row| {
                        row.hover(|style| style.bg(rgb(T.hover)))
                    })
                    // Mouse down, so the name is taken before the field can lose focus.
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        this.update(cx, |screen, cx| screen.pick_player(index, window, cx));
                    })
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(name.clone())),
                    )
                    .child(mono(format_count(*games)).flex_none().text_as(12., T.muted)),
            );
        }
        Some(
            gpui::deferred(gpui::anchored().child(div().mt(px(2.)).occlude().child(list)))
                .with_priority(1)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eco_ranges() {
        assert_eq!(parse_eco("b90–b99"), ("B90".to_owned(), "B99".to_owned()));
        assert_eq!(parse_eco("C50"), ("C50".to_owned(), "C50".to_owned()));
        assert_eq!(parse_eco("  "), (String::new(), String::new()));
    }
}
