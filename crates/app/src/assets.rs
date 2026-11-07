use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[exclude = "fonts/*"]
struct Bundled;

/// Fast's assets first, then the component library's icons.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Bundled::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items: Vec<SharedString> = Bundled::iter()
            .filter(|item| item.starts_with(path))
            .map(|item| item.into_owned().into())
            .collect();
        items.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(items)
    }
}

macro_rules! font {
    ($name:literal) => {
        Cow::Borrowed(include_bytes!(concat!("../../../assets/fonts/", $name)).as_slice())
    };
}

pub fn load_fonts(cx: &App) -> Result<()> {
    cx.text_system().add_fonts(vec![
        font!("IBMPlexSans-Regular.ttf"),
        font!("IBMPlexSans-Italic.ttf"),
        font!("IBMPlexSans-Medium.ttf"),
        font!("IBMPlexSans-SemiBold.ttf"),
        font!("IBMPlexSans-Bold.ttf"),
        font!("IBMPlexMono-Regular.ttf"),
        font!("IBMPlexMono-Medium.ttf"),
        font!("IBMPlexMono-SemiBold.ttf"),
        font!("IBMPlexMono-Bold.ttf"),
    ])
}
