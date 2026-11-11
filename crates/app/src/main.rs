//! Fast: a desktop chess workstation.

#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod analysis_store;
mod app_update;
mod assets;
mod backup;
mod config;
mod config_writer;
mod controls;
mod database_sources;
mod debug_window;
mod demo;
mod engine_catalog;
mod explorer;
mod fastchess;
mod game_download;
mod kibitz;
mod licenses;
mod live;
mod model;
mod online;
mod opening_books;
mod opening_data;
mod persistence;
mod process_guard;
mod recovery;
mod screens;
mod shell;
mod snapshot;
mod sound;
mod storage;
mod test_suite;
mod tournament_live;
mod tournament_wall;

use gpui::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    px, size,
};
use gpui_component::Root;

use crate::{model::AppModel, shell::Workspace};

/// Parsed command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    /// Run with the generated demo profile.
    pub demo: bool,
    /// Initial screen: analyse, database, train, play, tournament, settings[:section].
    pub screen: Option<String>,
    /// Draw the window controls even under server decorations (to test them on X11).
    pub client_decorations: bool,
    /// Initial window size, `WIDTHxHEIGHT`, for checking layouts at small sizes.
    pub size: Option<(u32, u32)>,
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
                "--size" => {
                    cli.size = args.next().and_then(|value| {
                        let (w, h) = value.split_once('x')?;
                        Some((w.parse().ok()?, h.parse().ok()?))
                    })
                }
                other => log::warn!("ignoring unknown argument {other}"),
            }
        }
        cli
    }
}

fn open_main_window(cli: Cli, cx: &mut App) -> anyhow::Result<()> {
    let model = cx.new(|_| AppModel::load(cli.demo));
    shell::refresh_menus(cx);
    let saved = model.read(cx).config.window.clone();
    let (width, height) = cli.size.unwrap_or_else(|| {
        saved.as_ref().map_or((1440, 900), |saved| {
            (saved.width.clamp(1100, 7680), saved.height.clamp(720, 4320))
        })
    });
    let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
    let maximized = cli.size.is_none() && saved.is_some_and(|saved| saved.maximized);
    let options = WindowOptions {
        app_id: Some("dev.fast.app".into()),
        window_bounds: Some(if maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }),
        window_min_size: Some(size(px(1100.), px(720.))),
        // Native decorations where the system offers them; otherwise `Workspace` draws its own.
        window_decorations: Some(WindowDecorations::Server),
        // On Windows the top bar is the title bar, so the menus share a line with the
        // window controls; Windows keeps the frame, resizing and snapping. On macOS the
        // top bar runs under a transparent title bar and holds the traffic lights.
        titlebar: Some(TitlebarOptions {
            title: Some("Fast".into()),
            appears_transparent: shell::DRAWS_TITLE_BAR || shell::MAC_TITLE_BAR,
            traffic_light_position: shell::MAC_TITLE_BAR.then(|| {
                let (x, y) = shell::TRAFFIC_LIGHTS_AT;
                gpui::point(px(x), px(y))
            }),
        }),
        ..Default::default()
    };
    let save_model = model.clone();
    cx.on_app_quit(move |cx| {
        let pending = save_model.update(cx, |model, cx| model.finish_config_save(cx));
        async move {
            if let Some(pending) = pending {
                pending.await;
            }
        }
    })
    .detach();
    cx.open_window(options, move |window, cx| {
        ui::set_theme(model.read(cx).config.ui_theme, cx);
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
    // Fast also runs as the guard that stops fastchess when Fast dies; see `process_guard`.
    if let Some(code) = process_guard::run_if_requested() {
        std::process::exit(code);
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse(std::env::args());
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            cx.set_app_identity("dev.fast.app", "Fast");
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
                size: None,
            }
        );
        assert_eq!(
            parse(&["fast", "--size", "1100x720"]).size,
            Some((1100, 720))
        );
        assert_eq!(parse(&["fast", "--size", "wide"]).size, None);
    }
}
