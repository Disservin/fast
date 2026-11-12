//! Editing a stored game's details.

use super::*;

/// A whole number typed in a field, or `None` when it is empty or unreadable.
pub(super) fn number(input: &Entity<InputState>, cx: &gpui::App) -> Option<i64> {
    input.read(cx).value().trim().parse().ok()
}

impl DatabaseScreen {
    /// Opens the editor for a game's players, ratings, result and other headers.
    pub(super) fn open_editor(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let row = match self.rows.get(&id) {
            Some(row) => row.clone(),
            None => match &self.preview {
                Some(preview) if preview.row.id == id => preview.row.clone(),
                _ => return,
            },
        };
        let info = GameInfo::from_row(&row);
        let elo = |elo: Option<i64>| elo.map(|elo| elo.to_string()).unwrap_or_default();
        let mut input =
            |value: String, placeholder: &'static str| new_input(value, placeholder, window, cx);
        let white = input(info.white, "");
        let black = input(info.black, "");
        let white_elo = input(elo(info.white_elo), "—");
        let black_elo = input(elo(info.black_elo), "—");
        let event = input(info.event, "");
        let round = input(info.round, "");
        let date = input(info.date, "YYYY-MM-DD");
        let eco = input(info.eco, "B90");
        let result = RESULTS
            .iter()
            .position(|result| *result == info.result)
            .unwrap_or(3);
        let mut editor = GameEditor {
            id,
            white,
            black,
            white_elo,
            black_elo,
            event,
            round,
            date,
            eco,
            result,
            _subscriptions: Vec::new(),
        };
        let inputs: Vec<_> = editor.inputs().into_iter().cloned().collect();
        for input in &inputs {
            editor
                ._subscriptions
                .push(cx.subscribe_in(input, window, |this, _, event, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.save_editor(cx);
                    }
                }));
        }
        editor.white.update(cx, |input, cx| input.focus(window, cx));
        self.editing = Some(editor);
        cx.notify();
    }

    /// Stores the edited headers and closes the editor.
    pub(super) fn save_editor(&mut self, cx: &mut Context<Self>) {
        let (Some(editor), Some(path)) = (&self.editing, self.selected_db.clone()) else {
            return;
        };
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let elo = |input: &Entity<InputState>| {
            let value = text(input);
            if value.is_empty() || value == "—" {
                Ok(None)
            } else {
                value.parse::<i64>().map(Some)
            }
        };
        let (Ok(white_elo), Ok(black_elo)) = (elo(&editor.white_elo), elo(&editor.black_elo))
        else {
            self.model.toast("Ratings must be whole numbers.", cx);
            return;
        };
        let info = GameInfo {
            white: text(&editor.white),
            black: text(&editor.black),
            white_elo,
            black_elo,
            result: RESULTS[editor.result].to_owned(),
            date: text(&editor.date).replace('.', "-"),
            event: text(&editor.event),
            round: text(&editor.round),
            eco: text(&editor.eco).to_uppercase(),
        };
        let id = editor.id;
        self.editing = None;
        cx.notify();
        let task = cx.background_spawn(async move {
            Database::open(&path).and_then(|db| db.update_game_info(id, &info))
        });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let result = task.await;
            model.update(cx, |model, cx| {
                match result {
                    Ok(()) => model.toast("Saved the game.", cx),
                    Err(err) => model.toast(format!("Could not save the game: {err}"), cx),
                }
                model.refresh_leaks(cx);
                cx.emit(ModelEvent::Databases);
            });
        })
        .detach();
    }

    /// The game editor over the tab: players, ratings, result, event, round, date and ECO.
    pub(super) fn render_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.editing.as_ref()?;
        let this = cx.entity();
        let result = choice(
            "edit-result",
            RESULTS
                .iter()
                .enumerate()
                .map(|(index, result)| (index, result_text(result))),
            editor.result,
            cx,
            |screen, index| {
                if let Some(editor) = &mut screen.editing {
                    editor.result = index;
                }
            },
        );
        let pair = |left: AnyElement, right: AnyElement| {
            div()
                .grid()
                .grid_cols(2)
                .gap_x(px(10.))
                .child(left)
                .child(right)
        };
        let input = |label: &'static str, state: &Entity<InputState>| {
            field(label, text_input(state).height(32.)).into_any_element()
        };
        let body = v_flex()
            .px(px(16.))
            .py(px(14.))
            .gap(px(10.))
            .child(pair(
                input("White", &editor.white),
                input("Black", &editor.black),
            ))
            .child(pair(
                input("White rating", &editor.white_elo),
                input("Black rating", &editor.black_elo),
            ))
            .child(field("Result", result))
            .child(pair(
                input("Event", &editor.event),
                input("Round", &editor.round),
            ))
            .child(pair(input("Date", &editor.date), input("ECO", &editor.eco)));
        let (cancel, save) = (this.clone(), this.clone());
        let footer = h_flex()
            .justify_end()
            .gap(px(6.))
            .px(px(16.))
            .py(px(12.))
            .border_t_as(T.border)
            .child(
                btn("edit-cancel", "Cancel").on_click(cancel.listen(move |screen, _, cx| {
                    screen.editing = None;
                    cx.notify();
                })),
            )
            .child(
                btn("edit-save", "Save")
                    .primary()
                    .on_click(save.listen(move |screen, _, cx| screen.save_editor(cx))),
            );
        Some(dialog(
            "game-editor",
            "Edit game",
            480.,
            body.child(footer).into_any_element(),
            this,
        ))
    }
}
