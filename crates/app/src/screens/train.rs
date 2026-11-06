use gpui::{Context, Entity, IntoElement, Render, SharedString, WeakEntity, Window, div};
use crate::{model::AppModel, shell::Workspace};
pub struct TrainScreen { _model: Entity<AppModel> }
impl TrainScreen {
    pub fn new(model: Entity<AppModel>, _w: WeakEntity<Workspace>, _window: &mut Window, _cx: &mut Context<Self>) -> Self { Self { _model: model } }
    pub fn breadcrumb(&self) -> SharedString { "".into() }
    pub fn search_player(&mut self, _q: &str, _w: &mut Window, _c: &mut Context<Self>) {}
    pub fn prompt_import(&mut self, _w: &mut Window, _c: &mut Context<Self>) {}
    pub fn prompt_open(&mut self, _w: &mut Window, _c: &mut Context<Self>) {}
    pub fn flip(&mut self, _c: &mut Context<Self>) {}
}
impl Render for TrainScreen { fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { div() } }
