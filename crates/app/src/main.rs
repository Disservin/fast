//! Fast: a desktop chess workstation.

mod assets;
mod config;
mod controls;
mod demo;
mod live;
mod model;
mod screens;
mod shell;
mod snapshot;

use gpui::{
    App, AppContext as _, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size,
};
use gpui_component::Root;

use crate::{model::AppModel, shell::Workspace};

/// Parsed command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    /// Run with the generated demo profile.
    pub demo: bool,
    /// Initial screen: analyse, database, train, play, settings[:section].
    pub screen: Option<String>,
    /// Draw the window controls even under server decorations (to test them on X11).
    pub client_decorations: bool,
}

impl Cli {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut cli = Self::default();
        let mut args = args.skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--demo" => cli.demo = true,
                "--screen" => cli.screen = args.next(),
                "--client-decorations" => cli.client_decorations = true,
                other => log::warn!("ignoring unknown argument {other}"),
            }
        }
        cli
    }
}

fn open_main_window(cli: Cli, cx: &mut App) -> anyhow::Result<()> {
    let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(1100.), px(720.))),
        // Native decorations where the system offers them; otherwise `Workspace` draws its own.
        window_decorations: Some(WindowDecorations::Server),
        ..Default::default()
    };
    cx.open_window(options, move |window, cx| {
        let model = cx.new(|_| AppModel::load(cli.demo));
        let workspace = cx.new(|cx| {
            let mut workspace = Workspace::new(model, cli.screen.as_deref(), window, cx);
            workspace.force_window_controls = cli.client_decorations;
            workspace
        });
        cx.new(|cx| Root::new(workspace, window, cx))
    })?;
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse(std::env::args());
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            ui::apply_component_theme(cx);
            if let Err(err) = assets::load_fonts(cx) {
                log::error!("could not load fonts: {err:#}");
            }
            shell::init(cx);
            if let Err(err) = open_main_window(cli, cx) {
                log::error!("could not open window: {err:#}");
                cx.quit();
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parsing() {
        let parse = |args: &[&str]| Cli::parse(args.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(parse(&["fast"]), Cli::default());
        assert_eq!(
            parse(&["fast", "--demo", "--screen", "train"]),
            Cli {
                demo: true,
                screen: Some("train".to_owned()),
                client_decorations: false,
            }
        );
    }
}
