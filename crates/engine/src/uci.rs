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
    pub pv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionKind {
    Check { default: bool },
    Spin { default: i64, min: i64, max: i64 },
    Combo { default: String, choices: Vec<String> },
    Button,
    String { default: String },
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
    Id { key: String, value: String },
    Option(UciOption),
    UciOk,
    ReadyOk,
    BestMove { best: Option<String>, ponder: Option<String> },
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
        let value = tokens.get(index).copied();
        match key {
            "depth" => update.depth = value?.parse().ok()?,
            "seldepth" => update.seldepth = value.and_then(|v| v.parse().ok()),
            "multipv" => update.multipv = value?.parse().ok()?,
            "nodes" => update.nodes = value.and_then(|v| v.parse().ok()),
            "nps" => update.nps = value.and_then(|v| v.parse().ok()),
            "tbhits" => update.tbhits = value.and_then(|v| v.parse().ok()),
            "score" => {
                let number = tokens.get(index + 1)?.parse().ok()?;
                update.score = match value? {
                    "cp" => Some(Score::Cp(number)),
                    "mate" => Some(Score::Mate(number)),
                    _ => None,
                };
                index += 2;
                // "upperbound" / "lowerbound" may follow; the loop ignores them.
                continue;
            }
            "wdl" => {
                update.wdl = Some(Wdl {
                    win: value?.parse().ok()?,
                    draw: tokens.get(index + 1)?.parse().ok()?,
                    loss: tokens.get(index + 2)?.parse().ok()?,
                });
                index += 3;
                continue;
            }
            "pv" => {
                update.pv = tokens[index..].iter().map(|&mv| mv.to_owned()).collect();
                break;
            }
            // Free text runs to the end of the line.
            "string" => return None,
            "time" | "hashfull" | "currmove" | "currmovenumber" | "cpuload" | "sbhits"
            | "refutation" | "currline" => {}
            _ => continue,
        }
        index += 1;
    }
    update.pv.first()?;
    Some(update)
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
            Some(Wdl { win: 345, draw: 512, loss: 143 })
        );
        assert_eq!(info.nps, Some(58_000_000));
        assert_eq!(info.tbhits, Some(7));
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
    }

    #[test]
    fn info_without_pv_or_with_string_is_ignored() {
        assert_eq!(parse_line("info depth 1 nodes 5"), UciLine::Other);
        assert_eq!(parse_line("info string NNUE loaded pv e2e4"), UciLine::Other);
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
            OptionKind::Spin { default: 20, min: 0, max: 20 }
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
        let UciLine::Option(string) = parse_line("option name SyzygyPath type string default <empty>")
        else {
            panic!();
        };
        assert_eq!(string.kind, OptionKind::String { default: String::new() });
        assert!(matches!(
            parse_line("option name Clear Hash type button"),
            UciLine::Option(UciOption { kind: OptionKind::Button, .. })
        ));
        assert!(matches!(
            parse_line("option name Ponder type check default false"),
            UciLine::Option(UciOption { kind: OptionKind::Check { default: false }, .. })
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
            UciLine::BestMove { best: None, ponder: None }
        );
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
