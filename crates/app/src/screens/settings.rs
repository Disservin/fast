use gpui::{Context, Entity, IntoElement, Render, Window, div};
use crate::model::AppModel;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsPage { General, Board, Engines, Databases, Shortcuts }
impl SettingsPage { pub fn title(self) -> &'static str { "General" } }
pub struct SettingsScreen { _model: Entity<AppModel>, page: SettingsPage }
impl SettingsScreen {
    pub fn new(model: Entity<AppModel>, _window: &mut Window, _cx: &mut Context<Self>) -> Self { Self { _model: model, page: SettingsPage::General } }
    pub fn page(&self) -> SettingsPage { self.page }
    pub fn set_page(&mut self, page: SettingsPage, _w: &mut Window, _c: &mut Context<Self>) { self.page = page; }
}
impl Render for SettingsScreen { fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { div() } }
