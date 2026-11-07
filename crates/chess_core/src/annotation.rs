//! Data embedded in PGN comments: Lichess/ChessBase `[%clk]`, `[%emt]` and `[%eval]`
//! commands, and TCEC's `d=, mt=, wv=, s=` fields. Used to draw graphs of games
//! that were already played.

use crate::{GameTree, NodeId};

/// What a move's comment says about it. Evals are White-relative centipawns,
/// with mates mapped to ±2000.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveAnnotation {
    /// Clock left after the move, in seconds.
    pub clock: Option<f64>,
    /// Time spent on the move, in seconds.
    pub elapsed: Option<f64>,
    pub eval_cp: Option<i32>,
    pub depth: Option<u32>,
    /// Engine speed in nodes per second.
    pub nps: Option<u64>,
}

/// Mates are drawn at this height on graphs.
pub const MATE_CP: i32 = 2000;

/// "1:02:03.5", "2:03", "45" → seconds.
fn parse_hms(text: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in text.trim().split(':') {
        let value: f64 = part.parse().ok()?;
        total = total * 60.0 + value;
    }
    Some(total)
}

/// The argument of a `[%name value]` command.
fn command<'a>(comment: &'a str, name: &str) -> Option<&'a str> {
    let tag = format!("[%{name}");
    let start = comment.find(&tag)? + tag.len();
    let rest = &comment[start..];
    let end = rest.find(']')?;
    Some(rest[..end].trim())
}

fn parse_eval_text(text: &str) -> Option<i32> {
    let text = text.split([',', ' ']).next()?.trim();
    if let Some(mate) = text.strip_prefix('#') {
        let moves: i32 = mate.parse().ok()?;
        return Some(if moves >= 0 { MATE_CP } else { -MATE_CP });
    }
    let pawns: f64 = text.parse().ok()?;
    Some((pawns * 100.0).round() as i32)
}

/// TCEC style `key=value,` fields.
fn field<'a>(comment: &'a str, key: &str) -> Option<&'a str> {
    comment.split([',', '{', '}']).find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name.trim() == key).then(|| value.trim())
    })
}

pub fn parse_annotation(comment: &str) -> MoveAnnotation {
    let mut out = MoveAnnotation {
        clock: command(comment, "clk").and_then(parse_hms),
        elapsed: command(comment, "emt").and_then(parse_hms),
        eval_cp: command(comment, "eval").and_then(parse_eval_text),
        ..Default::default()
    };
    out.depth = field(comment, "d").and_then(|value| value.parse().ok());
    if out.elapsed.is_none() {
        out.elapsed = field(comment, "mt")
            .and_then(|value| value.parse::<f64>().ok())
            .map(|ms| ms / 1000.0);
    }
    if out.clock.is_none() {
        out.clock = field(comment, "tl")
            .and_then(|value| value.parse::<f64>().ok())
            .map(|ms| ms / 1000.0);
    }
    if out.eval_cp.is_none() {
        out.eval_cp = field(comment, "wv").and_then(parse_eval_text);
    }
    // TCEC gives speed as "s=12345" in kN/s.
    out.nps = field(comment, "s")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|knps| (knps * 1000.0) as u64);
    out
}

/// A comment without its embedded `[%...]` commands, for display.
pub fn strip_commands(comment: &str) -> String {
    let mut out = String::with_capacity(comment.len());
    let mut rest = comment;
    while let Some(start) = rest.find("[%") {
        out.push_str(&rest[..start]);
        match rest[start..].find(']') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "180+2" → (180 s, 2 s); "40/7200:3600" and "-" are not understood.
pub fn parse_time_control(text: &str) -> Option<(f64, f64)> {
    let (base, inc) = text.split_once('+').unwrap_or((text, "0"));
    Some((base.trim().parse().ok()?, inc.trim().parse().ok()?))
}

/// Seconds spent on each move of `line`: from `[%emt]`/`mt=` when present, otherwise
/// from the drop in that side's clock, adding back the increment.
pub fn move_times(tree: &GameTree, line: &[NodeId]) -> Vec<Option<f64>> {
    let control = tree.header("TimeControl").and_then(parse_time_control);
    let increment = control.map_or(0.0, |(_, inc)| inc);
    // Last clock per side: index 0 for White.
    let mut last = [control.map(|(base, _)| base); 2];
    line.iter()
        .map(|&id| {
            let node = tree.node(id);
            let side = usize::from(node.ply.is_multiple_of(2));
            let note = parse_annotation(&node.comment);
            let spent = note.elapsed.or_else(|| {
                let clock = note.clock?;
                let before = last[side]?;
                Some((before - clock + increment).max(0.0))
            });
            if note.clock.is_some() {
                last[side] = note.clock;
            }
            spent
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_pgn;

    #[test]
    fn lichess_commands() {
        let note = parse_annotation("[%eval 0.17] [%clk 0:02:58]");
        assert_eq!(note.eval_cp, Some(17));
        assert_eq!(note.clock, Some(178.0));
        assert_eq!(parse_annotation("[%eval #-3]").eval_cp, Some(-MATE_CP));
        assert_eq!(parse_annotation("[%emt 0:00:07.5]").elapsed, Some(7.5));
        assert_eq!(
            parse_annotation("a plain comment"),
            MoveAnnotation::default()
        );
    }

    #[test]
    fn tcec_fields() {
        let note = parse_annotation(
            "d=34, sd=52, mt=61000, tl=5400000, s=78123, n=4771234, pv=Nf3 d5, tb=0, wv=0.31,",
        );
        assert_eq!(note.depth, Some(34));
        assert_eq!(note.elapsed, Some(61.0));
        assert_eq!(note.clock, Some(5400.0));
        assert_eq!(note.eval_cp, Some(31));
        assert_eq!(note.nps, Some(78_123_000));
    }

    #[test]
    fn commands_are_hidden_from_display() {
        assert_eq!(
            strip_commands("Good move [%clk 0:01:00] really [%eval 1.0]"),
            "Good move really"
        );
        assert_eq!(strip_commands("[%clk 0:01:00]"), "");
    }

    #[test]
    fn times_come_from_clock_drops_with_increment() {
        let pgn = "[TimeControl \"60+1\"]\n\n1. e4 {[%clk 0:01:00]} e5 {[%clk 0:00:58]} \
                   2. Nf3 {[%clk 0:00:51]} Nc6 {[%emt 0:00:03]} *";
        let tree = parse_pgn(pgn).unwrap().remove(0);
        let times = move_times(&tree, &tree.mainline());
        assert_eq!(times, vec![Some(1.0), Some(3.0), Some(10.0), Some(3.0)]);
        assert_eq!(parse_time_control("180+2"), Some((180.0, 2.0)));
        assert_eq!(parse_time_control("300"), Some((300.0, 0.0)));
        assert_eq!(parse_time_control("-"), None);
    }
}
