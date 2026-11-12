//! The summary of a tournament's settings, read back from its fastchess command.

use super::*;

/// What a tournament was started with, read back from its command.
pub(super) fn locked_summary(run: &Run) -> Div {
    let args = &run.args;
    let engines = engines(args);
    let names: Vec<String> = engines.iter().map(|(name, _)| name.clone()).collect();
    let mut list = v_flex().gap(px(10.));
    for (name, cmd) in &engines {
        list = list.child(
            h_flex()
                .gap(px(10.))
                .items_start()
                .child(swatch(engine_colour(&names, name)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(11.))
                                .text_color(rgb(T.muted))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(cmd.clone())),
                        ),
                ),
        );
    }

    let each = flag_values(args, "-each");
    let limit = ["tc", "st", "nodes", "depth"]
        .into_iter()
        .find_map(|key| {
            kv(&each, key).map(|value| {
                if key == "tc" {
                    value.to_owned()
                } else {
                    format!("{key} {value}")
                }
            })
        })
        .unwrap_or_else(|| "–".into());
    let book = kv(&flag_values(args, "-openings"), "file")
        .map(|file| {
            Path::new(file)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.to_owned())
        })
        .unwrap_or_else(|| "None".into());
    let schedule = format!(
        "{}, {}",
        if flag_value(args, "-tournament") == Some("gauntlet") {
            "Gauntlet"
        } else {
            "Round robin"
        },
        if has(args, "-repeat") {
            "2 games per round"
        } else {
            "1 game per round"
        }
    );
    let settings = [
        ("Time control", limit),
        (
            "Hash",
            format!("{} MB", kv(&each, "option.Hash").unwrap_or("–")),
        ),
        (
            "Threads",
            kv(&each, "option.Threads").unwrap_or("–").to_owned(),
        ),
        ("Schedule", schedule),
        (
            "Rounds",
            flag_value(args, "-rounds").unwrap_or("–").to_owned(),
        ),
        (
            "Concurrency",
            flag_value(args, "-concurrency").unwrap_or("–").to_owned(),
        ),
        ("Book", book),
    ];
    let mut defs = v_flex().gap(px(8.));
    for (label, value) in settings {
        defs = defs.child(def_row(label, value));
    }
    let on_off = |flag: &str| if has(args, flag) { "On" } else { "Off" };
    let sprt = if has(args, "-sprt") {
        let sprt = flag_values(args, "-sprt");
        format!(
            "elo0 {}, elo1 {}",
            kv(&sprt, "elo0").unwrap_or("–"),
            kv(&sprt, "elo1").unwrap_or("–")
        )
    } else {
        "Off".to_owned()
    };
    let draw = if has(args, "-draw") {
        let draw = flag_values(args, "-draw");
        format!(
            "From move {}, {} moves within ±{} cp",
            kv(&draw, "movenumber").unwrap_or("–"),
            kv(&draw, "movecount").unwrap_or("–"),
            kv(&draw, "score").unwrap_or("–")
        )
    } else {
        "Off".to_owned()
    };
    let resign = if has(args, "-resign") {
        let resign = flag_values(args, "-resign");
        format!(
            "{} moves beyond {} cp{}",
            kv(&resign, "movecount").unwrap_or("–"),
            kv(&resign, "score").unwrap_or("–"),
            if kv(&resign, "twosided") == Some("true") {
                ", both engines"
            } else {
                ""
            }
        )
    } else {
        "Off".to_owned()
    };
    let mut rules = v_flex().gap(px(8.));
    for (label, value) in [
        ("Draw adjudication", draw),
        ("Resign adjudication", resign),
        ("Recover crashes", on_off("-recover").to_owned()),
        ("SPRT", sprt),
    ] {
        rules = rules.child(def_row(label, value));
    }

    v_flex()
        .w_full()
        .gap(px(20.))
        .child(
            card()
                .w_full()
                .gap(px(12.))
                .child(heading("Engines"))
                .child(list),
        )
        .child(
            card()
                .w_full()
                .gap(px(12.))
                .child(heading("Settings"))
                .child(defs)
                .child(divider())
                .child(rules),
        )
}
