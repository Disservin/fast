//! Typed parsing of the UCI protocol lines an engine writes to stdout.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Score {
    /// Centipawns from White's point of view once normalised.
    Cp(i32),
    Mate(i32),
}

impl Score {
    /// Flips the sign (UCI scores are side-to-move relative).
    pub fn negate(self) -> Self {
        match self {
            Self::Cp(cp) => Self::Cp(-cp),
            Self::Mate(mate) => Self::Mate(-mate),
        }
    }

    /// A single comparable number; mates rank above any centipawn score and
    /// shorter mates rank above longer ones.
    pub fn sort_key(self) -> i32 {
        match self {
            Self::Cp(cp) => cp,
            Self::Mate(mate) if mate > 0 => 100_000 - mate,
            Self::Mate(mate) => -100_000 - mate,
        }
    }

    /// "+0.42", "-1.05", "#3", "#-2".
    pub fn display(self) -> String {
        match self {
            Self::Cp(cp) => format!("{:+.2}", f64::from(cp) / 100.0),
            Self::Mate(mate) => format!("#{mate}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wdl {
    pub win: u32,
    pub draw: u32,
    pub loss: u32,
}

impl Wdl {
    pub fn flip(self) -> Self {
        Self {
            win: self.loss,
            draw: self.draw,
            loss: self.win,
        }
    }
}

/// One `info` line carrying a principal variation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalysisUpdate {
    pub depth: u32,
    pub seldepth: Option<u32>,
    pub multipv: u32,
    pub score: Option<Score>,
    pub wdl: Option<Wdl>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub tbhits: Option<u64>,
    /// Transposition table use, in permille.
    pub hashfull: Option<u32>,
    pub time_ms: Option<u64>,
    /// The score is only an upper or lower bound: the search failed high or low and
    /// `pv` is usually just the move being tried, not a real line.
    pub bound: bool,
    pub pv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionKind {
    Check {
        default: bool,
    },
    Spin {
        default: i64,
        min: i64,
        max: i64,
    },
    Combo {
        default: String,
        choices: Vec<String>,
    },
    Button,
    String {
        default: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UciOption {
    pub name: String,
    pub kind: OptionKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    pub name: String,
    pub author: String,
    pub options: Vec<UciOption>,
}

/// Parsed engine output. Unknown lines are preserved as `Other`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UciLine {
    Id {
        key: String,
        value: String,
    },
    Option(UciOption),
    UciOk,
    ReadyOk,
    BestMove {
        best: Option<String>,
        ponder: Option<String>,
    },
    Info(AnalysisUpdate),
    Other,
}

pub fn parse_line(line: &str) -> UciLine {
    let line = line.trim();
    let mut tokens = line.split_ascii_whitespace();
    match tokens.next() {
        Some("uciok") => UciLine::UciOk,
        Some("readyok") => UciLine::ReadyOk,
        Some("id") => {
            let Some(key) = tokens.next() else {
                return UciLine::Other;
            };
            UciLine::Id {
                key: key.to_owned(),
                value: tokens.collect::<Vec<_>>().join(" "),
            }
        }
        Some("option") => parse_option(line).map_or(UciLine::Other, UciLine::Option),
        Some("bestmove") => {
            let best = tokens
                .next()
                .filter(|mv| !matches!(*mv, "(none)" | "0000"))
                .map(str::to_owned);
            let ponder = match tokens.next() {
                Some("ponder") => tokens.next().map(str::to_owned),
                _ => None,
            };
            UciLine::BestMove { best, ponder }
        }
        Some("info") => parse_info(line).map_or(UciLine::Other, UciLine::Info),
        _ => UciLine::Other,
    }
}

/// Every field an `info` line can carry. A field's value runs up to the next of these.
const INFO_KEYWORDS: [&str; 20] = [
    "depth",
    "seldepth",
    "time",
    "nodes",
    "pv",
    "multipv",
    "score",
    "wdl",
    "currmove",
    "currmovenumber",
    "hashfull",
    "nps",
    "tbhits",
    "sbhits",
    "cpuload",
    "string",
    "refutation",
    "currline",
    "upperbound",
    "lowerbound",
];

fn parse_info(line: &str) -> Option<AnalysisUpdate> {
    let tokens: Vec<&str> = line.split_ascii_whitespace().skip(1).collect();
    let mut update = AnalysisUpdate {
        multipv: 1,
        ..Default::default()
    };
    let mut index = 0;
    while index < tokens.len() {
        let key = tokens[index];
        index += 1;
        // The field's own tokens, up to the next keyword. `currmove`, `currline` and
        // `refutation` carry moves too, but they are not the principal variation.
        let end = tokens[index..]
            .iter()
            .position(|token| INFO_KEYWORDS.contains(token))
            .map_or(tokens.len(), |offset| index + offset);
        let values = &tokens[index..end];
        let first = values.first().copied();
        match key {
            "depth" => update.depth = first?.parse().ok()?,
            "seldepth" => update.seldepth = first.and_then(|v| v.parse().ok()),
            "multipv" => update.multipv = first?.parse().ok()?,
            "nodes" => update.nodes = first.and_then(|v| v.parse().ok()),
            "nps" => update.nps = first.and_then(|v| v.parse().ok()),
            "tbhits" => update.tbhits = first.and_then(|v| v.parse().ok()),
            "hashfull" => update.hashfull = first.and_then(|v| v.parse().ok()),
            "time" => update.time_ms = first.and_then(|v| v.parse().ok()),
            "score" => {
                let number = values.get(1)?.parse().ok()?;
                update.score = match first? {
                    "cp" => Some(Score::Cp(number)),
                    "mate" => Some(Score::Mate(number)),
                    _ => None,
                };
            }
            "upperbound" | "lowerbound" => update.bound = true,
            "wdl" => {
                update.wdl = Some(Wdl {
                    win: values.first()?.parse().ok()?,
                    draw: values.get(1)?.parse().ok()?,
                    loss: values.get(2)?.parse().ok()?,
                });
            }
            "pv" => update.pv = values.iter().map(|&mv| mv.to_owned()).collect(),
            // Free text runs to the end of the line.
            "string" => return None,
            _ => {}
        }
        index = end;
    }
    update.pv.first()?;
    Some(update)
}

/// Node count with a metric suffix: "950 n", "12.4 kn", "1.20 Gn".
pub fn format_nodes(nodes: u64) -> String {
    let value = nodes as f64;
    match nodes {
        0..1_000 => format!("{nodes} n"),
        1_000..1_000_000 => format!("{:.1} kn", value / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} Mn", value / 1e6),
        _ => format!("{:.2} Gn", value / 1e9),
    }
}

/// Search speed: "850 n/s", "412 kn/s", "58.0 Mn/s".
pub fn format_speed(nps: u64) -> String {
    let value = nps as f64;
    match nps {
        0..1_000 => format!("{nps} n/s"),
        1_000..1_000_000 => format!("{:.0} kn/s", value / 1e3),
        _ => format!("{:.1} Mn/s", value / 1e6),
    }
}

const OPTION_KEYWORDS: [&str; 7] = ["name", "type", "default", "min", "max", "var", "option"];

fn parse_option(line: &str) -> Option<UciOption> {
    let tokens: Vec<&str> = line.split_ascii_whitespace().skip(1).collect();
    let mut name = None;
    let mut kind = None;
    let mut default = None;
    let mut min = None;
    let mut max = None;
    let mut vars = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let key = tokens[index];
        let start = index + 1;
        let mut end = start;
        // Values may contain spaces ("Skill Level"), up to the next keyword.
        while end < tokens.len() && !OPTION_KEYWORDS.contains(&tokens[end]) {
            end += 1;
        }
        let value = tokens[start..end].join(" ");
        match key {
            "name" => name = Some(value),
            "type" => kind = Some(value),
            "default" => default = Some(value),
            "min" => min = value.parse().ok(),
            "max" => max = value.parse().ok(),
            "var" => vars.push(value),
            _ => {}
        }
        index = end;
    }
    let default_text = default.unwrap_or_default();
    let kind = match kind?.as_str() {
        "check" => OptionKind::Check {
            default: default_text == "true",
        },
        "spin" => OptionKind::Spin {
            default: default_text.parse().unwrap_or(0),
            min: min.unwrap_or(i64::MIN),
            max: max.unwrap_or(i64::MAX),
        },
        "combo" => OptionKind::Combo {
            default: default_text,
            choices: vars,
        },
        "button" => OptionKind::Button,
        "string" => OptionKind::String {
            default: if default_text == "<empty>" {
                String::new()
            } else {
                default_text
            },
        },
        _ => return None,
    };
    Some(UciOption { name: name?, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_info_line() {
        let line = "info depth 34 seldepth 48 multipv 2 score cp 42 wdl 345 512 143 nodes 1000 nps 58000000 hashfull 12 tbhits 7 time 100 pv f3f4 a7a5 f4f5";
        let UciLine::Info(info) = parse_line(line) else {
            panic!("not info");
        };
        assert_eq!(info.depth, 34);
        assert_eq!(info.seldepth, Some(48));
        assert_eq!(info.multipv, 2);
        assert_eq!(info.score, Some(Score::Cp(42)));
        assert_eq!(
            info.wdl,
            Some(Wdl {
                win: 345,
                draw: 512,
                loss: 143
            })
        );
        assert_eq!(info.nps, Some(58_000_000));
        assert_eq!(info.tbhits, Some(7));
        assert_eq!(info.hashfull, Some(12));
        assert_eq!(info.time_ms, Some(100));
        assert_eq!(info.pv, vec!["f3f4", "a7a5", "f4f5"]);
    }

    #[test]
    fn parses_mate_and_bounds() {
        let UciLine::Info(info) =
            parse_line("info depth 5 score mate -3 lowerbound nodes 9 pv e2e4")
        else {
            panic!("not info");
        };
        assert_eq!(info.score, Some(Score::Mate(-3)));
        assert_eq!(info.nodes, Some(9));
        assert!(info.bound);
    }

    #[test]
    fn pv_stops_at_the_next_field() {
        let UciLine::Info(info) = parse_line(
            "info depth 12 score cp 20 pv e2e4 e7e5 currmove d2d4 currmovenumber 2 nodes 5",
        ) else {
            panic!("not info");
        };
        assert_eq!(info.pv, vec!["e2e4", "e7e5"]);
        assert_eq!(info.nodes, Some(5));
        assert!(!info.bound);
    }

    #[test]
    fn current_move_lines_are_not_lines() {
        assert_eq!(
            parse_line("info depth 30 currmove e2e4 currmovenumber 1"),
            UciLine::Other
        );
        let UciLine::Info(info) =
            parse_line("info depth 12 currmove d2d4 currline 1 d2d4 d7d5 pv e2e4 g8f6")
        else {
            panic!("not info");
        };
        assert_eq!(info.pv, vec!["e2e4", "g8f6"]);
    }

    #[test]
    fn info_without_pv_or_with_string_is_ignored() {
        assert_eq!(parse_line("info depth 1 nodes 5"), UciLine::Other);
        assert_eq!(
            parse_line("info string NNUE loaded pv e2e4"),
            UciLine::Other
        );
    }

    #[test]
    fn parses_option_kinds() {
        let UciLine::Option(spin) =
            parse_line("option name Skill Level type spin default 20 min 0 max 20")
        else {
            panic!();
        };
        assert_eq!(spin.name, "Skill Level");
        assert_eq!(
            spin.kind,
            OptionKind::Spin {
                default: 20,
                min: 0,
                max: 20
            }
        );
        let UciLine::Option(combo) =
            parse_line("option name Style type combo default Normal var Solid var Very Risky")
        else {
            panic!();
        };
        assert_eq!(
            combo.kind,
            OptionKind::Combo {
                default: "Normal".to_owned(),
                choices: vec!["Solid".to_owned(), "Very Risky".to_owned()],
            }
        );
        let UciLine::Option(string) =
            parse_line("option name SyzygyPath type string default <empty>")
        else {
            panic!();
        };
        assert_eq!(
            string.kind,
            OptionKind::String {
                default: String::new()
            }
        );
        assert!(matches!(
            parse_line("option name Clear Hash type button"),
            UciLine::Option(UciOption {
                kind: OptionKind::Button,
                ..
            })
        ));
        assert!(matches!(
            parse_line("option name Ponder type check default false"),
            UciLine::Option(UciOption {
                kind: OptionKind::Check { default: false },
                ..
            })
        ));
    }

    #[test]
    fn parses_bestmove_variants() {
        assert_eq!(
            parse_line("bestmove e2e4 ponder e7e5"),
            UciLine::BestMove {
                best: Some("e2e4".to_owned()),
                ponder: Some("e7e5".to_owned())
            }
        );
        assert_eq!(
            parse_line("bestmove (none)"),
            UciLine::BestMove {
                best: None,
                ponder: None
            }
        );
    }

    #[test]
    fn node_and_speed_formatting() {
        assert_eq!(format_nodes(950), "950 n");
        assert_eq!(format_nodes(12_400), "12.4 kn");
        assert_eq!(format_nodes(3_500_000), "3.5 Mn");
        assert_eq!(format_nodes(1_200_000_000), "1.20 Gn");
        assert_eq!(format_speed(850), "850 n/s");
        assert_eq!(format_speed(412_000), "412 kn/s");
        assert_eq!(format_speed(58_000_000), "58.0 Mn/s");
    }

    #[test]
    fn score_display_and_order() {
        assert_eq!(Score::Cp(42).display(), "+0.42");
        assert_eq!(Score::Cp(-105).display(), "-1.05");
        assert_eq!(Score::Mate(3).display(), "#3");
        assert!(Score::Mate(1).sort_key() > Score::Mate(5).sort_key());
        assert!(Score::Mate(5).sort_key() > Score::Cp(5000).sort_key());
        assert!(Score::Mate(-5).sort_key() > Score::Mate(-1).sort_key());
    }
}
