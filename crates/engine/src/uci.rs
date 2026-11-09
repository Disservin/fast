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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

impl EngineInfo {
    /// The engine's self-description from its `UCI_EngineAbout` option, if offered.
    pub fn about(&self) -> Option<&str> {
        self.options.iter().find_map(|option| match &option.kind {
            OptionKind::String { default } if option.name == "UCI_EngineAbout" => {
                Some(default.as_str()).filter(|text| !text.trim().is_empty())
            }
            _ => None,
        })
    }
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

/// Every field an `info` line can carry. Anything else is a field from a newer or
/// engine-specific protocol, skipped up to the next field named here.
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

/// A move in UCI notation: `e2e4`, `e7e8q`, a Chess960 king-takes-rook castle, a
/// drop such as `N@f3`, or the null move `0000`.
fn is_uci_move(token: &str) -> bool {
    let bytes = token.as_bytes();
    let square =
        |file: u8, rank: u8| (b'a'..=b'h').contains(&file) && (b'1'..=b'8').contains(&rank);
    match bytes {
        b"0000" => true,
        [piece, b'@', file, rank] => b"PNBRQKpnbrqk".contains(piece) && square(*file, *rank),
        [from_file, from_rank, to_file, to_rank] => {
            square(*from_file, *from_rank) && square(*to_file, *to_rank)
        }
        [from_file, from_rank, to_file, to_rank, promotion] => {
            square(*from_file, *from_rank)
                && square(*to_file, *to_rank)
                && b"qrbnkQRBNK".contains(promotion)
        }
        _ => false,
    }
}

/// A number written as an integer, or as a decimal that a newer engine might send,
/// rounded to the nearest whole value.
fn parse_number<T: std::str::FromStr + TryFrom<i64>>(token: &str) -> Option<T> {
    token.parse().ok().or_else(|| {
        let value: f64 = token.parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        T::try_from(value.round() as i64).ok()
    })
}

/// Reads an `info` line. Fields may come in any order, unknown fields are skipped,
/// and a field that cannot be read is left out rather than losing the whole line;
/// only a line without a principal variation is not analysis.
fn parse_info(line: &str) -> Option<AnalysisUpdate> {
    let tokens: Vec<&str> = line.split_ascii_whitespace().skip(1).collect();
    let mut update = AnalysisUpdate {
        multipv: 1,
        ..Default::default()
    };
    let mut index = 0;
    // The value right after a field name, if it is not itself a field name.
    let value = |index: usize| {
        tokens
            .get(index)
            .copied()
            .filter(|token| !INFO_KEYWORDS.contains(token))
    };
    let moves_from = |mut index: usize| {
        while tokens.get(index).is_some_and(|token| is_uci_move(token)) {
            index += 1;
        }
        index
    };
    while index < tokens.len() {
        let key = tokens[index];
        index += 1;
        match key {
            "depth" | "seldepth" | "multipv" | "nodes" | "nps" | "tbhits" | "hashfull" | "time"
            | "sbhits" | "cpuload" | "currmovenumber" | "currmove" => {
                let Some(text) = value(index) else { continue };
                index += 1;
                match key {
                    "depth" => update.depth = parse_number(text).unwrap_or(update.depth),
                    "seldepth" => update.seldepth = parse_number(text),
                    "multipv" => update.multipv = parse_number(text).unwrap_or(1).max(1),
                    "nodes" => update.nodes = parse_number(text),
                    "nps" => update.nps = parse_number(text),
                    "tbhits" => update.tbhits = parse_number(text),
                    "hashfull" => update.hashfull = parse_number(text),
                    "time" => update.time_ms = parse_number(text),
                    _ => {}
                }
            }
            // `score cp 20`, `score mate -3`, either followed by a bound, in any
            // order a newer engine might use.
            "score" => {
                while let Some(part) = tokens.get(index).copied() {
                    match part {
                        "cp" | "mate" => {
                            index += 1;
                            let Some(number) = tokens.get(index).and_then(|n| parse_number(n))
                            else {
                                continue;
                            };
                            index += 1;
                            update.score = Some(if part == "cp" {
                                Score::Cp(number)
                            } else {
                                Score::Mate(number)
                            });
                        }
                        "upperbound" | "lowerbound" => {
                            index += 1;
                            update.bound = true;
                        }
                        _ => break,
                    }
                }
            }
            "upperbound" | "lowerbound" => update.bound = true,
            "wdl" => {
                let numbers: Vec<u32> = tokens[index..]
                    .iter()
                    .take(3)
                    .map_while(|token| parse_number(token))
                    .collect();
                index += numbers.len();
                if let [win, draw, loss] = numbers[..] {
                    update.wdl = Some(Wdl { win, draw, loss });
                }
            }
            // Only moves belong to the line, so whatever follows them is read as the
            // next field even if this parser has never heard of it.
            "pv" => {
                let end = moves_from(index);
                update.pv = tokens[index..end].iter().map(|&mv| mv.to_owned()).collect();
                index = end;
            }
            "refutation" => index = moves_from(index),
            // An optional CPU number, then moves.
            "currline" => {
                if value(index).is_some_and(|token| token.parse::<u32>().is_ok()) {
                    index += 1;
                }
                index = moves_from(index);
            }
            // Free text runs to the end of the line.
            "string" => break,
            _ => {
                while value(index).is_some() {
                    index += 1;
                }
            }
        }
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

/// A hash size given in MB, in GB from 1024 MB on: "512 MB", "16 GB", "9.8 GB".
pub fn format_hash(mb: i64) -> String {
    if mb < 1024 {
        return format!("{mb} MB");
    }
    let gb = mb as f64 / 1024.;
    if mb % 1024 == 0 {
        format!("{gb:.0} GB")
    } else {
        format!("{gb:.1} GB")
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
    fn unknown_and_future_fields_are_skipped() {
        let UciLine::Info(info) = parse_line(
            "info depth 20 movesleft 31 score cp 15 wdl 300 600 100 nodes 10 pv e2e4 e7e5 effort 12 7",
        ) else {
            panic!("not info");
        };
        assert_eq!(info.depth, 20);
        assert_eq!(info.score, Some(Score::Cp(15)));
        assert_eq!(info.nodes, Some(10));
        assert_eq!(info.pv, vec!["e2e4", "e7e5"]);
        // Fields in another order, a bound before the score, and decimals.
        let UciLine::Info(info) =
            parse_line("info pv g1f3 multipv 2 score lowerbound cp 33.6 depth 7 time 12.0")
        else {
            panic!("not info");
        };
        assert_eq!(info.pv, vec!["g1f3"]);
        assert_eq!(info.multipv, 2);
        assert_eq!(info.score, Some(Score::Cp(34)));
        assert!(info.bound);
        assert_eq!(info.depth, 7);
        assert_eq!(info.time_ms, Some(12));
    }

    #[test]
    fn a_bad_field_does_not_lose_the_line() {
        let UciLine::Info(info) = parse_line(
            "info depth x seldepth -1 multipv score cp 5 pv e7e8q e1h1 N@f3 string done",
        ) else {
            panic!("not info");
        };
        assert_eq!(info.depth, 0);
        assert_eq!(info.seldepth, None);
        assert_eq!(info.multipv, 1);
        assert_eq!(info.score, Some(Score::Cp(5)));
        assert_eq!(info.pv, vec!["e7e8q", "e1h1", "N@f3"]);
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
    fn engine_about_comes_from_its_option() {
        let UciLine::Option(option) = parse_line(
            "option name UCI_EngineAbout type string default Stockfish by the Stockfish developers see https://stockfishchess.org",
        ) else {
            panic!();
        };
        let mut info = EngineInfo::default();
        assert_eq!(info.about(), None);
        info.options.push(option);
        assert_eq!(
            info.about(),
            Some("Stockfish by the Stockfish developers see https://stockfishchess.org")
        );
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
        assert_eq!(format_hash(512), "512 MB");
        assert_eq!(format_hash(16_384), "16 GB");
        assert_eq!(format_hash(10_000), "9.8 GB");
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
