//! The demo engines and their scripted analysis.

use super::*;

pub(super) fn demo_engine(
    id: u64,
    (name, version, path): (&str, &str, &str),
    enabled: bool,
    uses: (bool, bool, bool),
    options: &[(&str, &str)],
) -> EngineConfig {
    let mut schema = vec![
        spin("Threads", 16, 1, 1024),
        spin("Hash", 4096, 1, 1_048_576),
        spin("MultiPV", 3, 1, 256),
        UciOption {
            name: "SyzygyPath".to_owned(),
            kind: OptionKind::String {
                default: String::new(),
            },
        },
        UciOption {
            name: "UCI_ShowWDL".to_owned(),
            kind: OptionKind::Check { default: true },
        },
    ];
    if name == "Stockfish" {
        schema.push(UciOption {
            name: "UCI_Chess960".to_owned(),
            kind: OptionKind::Check { default: false },
        });
    }
    EngineConfig {
        id,
        name: name.to_owned(),
        version: version.to_owned(),
        path: path.into(),
        enabled,
        options: options
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        use_analysis: uses.0,
        use_review: uses.1,
        use_play: uses.2,
        info: EngineInfo {
            name: format!("{name} {version}"),
            author: String::new(),
            options: schema,
        },
        ..Default::default()
    }
}

pub(super) fn spin(name: &str, default: i64, min: i64, max: i64) -> UciOption {
    UciOption {
        name: name.to_owned(),
        kind: OptionKind::Spin { default, min, max },
    }
}

/// Timeline evals for the sample game, as in the design.
pub const TIMELINE: [i32; 26] = [
    14, 20, 17, 20, 14, 25, 20, 20, 17, 25, 20, 25, 20, 14, 17, 20, 25, 14, 42, 20, 75, 6, 25, -55,
    20, 42,
];

/// Scripted engine output for the sample position. Returns per-engine-name lines.
pub fn scripted_lines(hash: u64) -> HashMap<&'static str, Vec<AnalysisUpdate>> {
    let tree = chess_core::sample::najdorf();
    let end = *tree.mainline().last().unwrap_or(&GameTree::ROOT);
    let mut out = HashMap::new();
    if chess_core::position_hash(&tree.node(end).pos) != hash {
        return out;
    }
    let update = |multipv, cp, wdl: (u32, u32, u32), pv: &str| AnalysisUpdate {
        depth: 34,
        seldepth: Some(52),
        multipv,
        score: Some(Score::Cp(cp)),
        wdl: Some(Wdl {
            win: wdl.0,
            draw: wdl.1,
            loss: wdl.2,
        }),
        nodes: Some(1_200_000_000),
        nps: Some(58_000_000),
        tbhits: None,
        hashfull: Some(214),
        time_ms: Some(21_000),
        bound: false,
        pv: pv.split(' ').map(str::to_owned).collect(),
    };
    let f4 = "f3f4 a6a5 f4f5 e6c4 c1b1 a5a4 b3c1 c4e2 f1e2 a4a3 b2b3";
    let h4 = "h2h4 a6a5 c1b1 a5a4 b3c1 d6d5";
    let kb1 = "c1b1 a6a5 c2c3 b4c3 e2c3 a5a4";
    let ng3 = "e2g3 a6a5 c1b1 a5a4 b3c1 g7g6";
    let lines = vec![
        update(1, 42, (330, 520, 150), f4),
        update(2, 31, (310, 520, 170), h4),
        update(3, 18, (280, 560, 160), kb1),
        update(4, 5, (240, 550, 210), ng3),
    ];
    out.insert("Lc0", lines.clone());
    out.insert("Stockfish", lines);
    out
}

/// A scripted line written the way an engine would, for the Log panel.
pub fn info_line(update: &AnalysisUpdate) -> String {
    let score = match update.score {
        Some(Score::Cp(cp)) => format!("cp {cp}"),
        Some(Score::Mate(mate)) => format!("mate {mate}"),
        None => "cp 0".to_owned(),
    };
    format!(
        "info depth {} seldepth {} multipv {} score {score} nodes {} nps {} pv {}",
        update.depth,
        update.seldepth.unwrap_or(0),
        update.multipv,
        update.nodes.unwrap_or(0),
        update.nps.unwrap_or(0),
        update.pv.join(" "),
    )
}

/// One-line explanation shown in Line preview (the hook is empty outside demo mode).
pub fn explanation(san: &str) -> Option<&'static str> {
    (san == "f4").then_some(
        "Gains a tempo on the kingside and forces Black to commit the light-squared bishop before the queenside storm lands.",
    )
}

/// A mid-game Italian for the Play screen.
pub const PLAY_LINE: &str = "e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6";
