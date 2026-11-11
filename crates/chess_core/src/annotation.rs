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
    /// White-relative moves to mate, preserved for move-list labels and PGN export.
    pub mate_distance: Option<i32>,
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

/// Reads a signed mate distance without losing it to the graph's mate sentinel.
fn parse_mate_distance(text: &str) -> Option<i32> {
    text.split([',', ' '])
        .next()?
        .trim()
        .strip_prefix('#')?
        .parse()
        .ok()
}

fn parse_eval_text(text: &str) -> Option<i32> {
    let text = text.split([',', ' ']).next()?.trim();
    if let Some(mate) = text.strip_prefix('#') {
        let moves: i32 = mate.parse().ok()?;
        return Some(if moves >= 0 { MATE_CP } else { -MATE_CP });
    }
    let pawns: f64 = text.parse().ok()?;
    pawns.is_finite().then(|| (pawns * 100.0).round() as i32)
}

/// Cute Chess writes `score/depth seconds`, with score relative to the mover.
fn cutechess(comment: &str) -> Option<MoveAnnotation> {
    let mut words = comment.split_whitespace();
    let (score, depth) = words.next()?.split_once('/')?;
    let depth = depth.parse().ok()?;
    let mate_distance = if let Some(mate) = score.strip_prefix("-M") {
        Some(-mate.parse::<i32>().ok()?)
    } else if let Some(mate) = score.strip_prefix("+M").or_else(|| score.strip_prefix('M')) {
        Some(mate.parse::<i32>().ok()?)
    } else {
        None
    };
    let score = if let Some(mate) = score.strip_prefix("-M") {
        mate.parse::<u32>().ok()?;
        -MATE_CP
    } else if let Some(mate) = score.strip_prefix("+M").or_else(|| score.strip_prefix('M')) {
        mate.parse::<u32>().ok()?;
        MATE_CP
    } else {
        parse_eval_text(score)?
    };
    let elapsed = words.next().and_then(|time| {
        let seconds = time.trim_end_matches('s').parse::<f64>().ok()?;
        (seconds.is_finite() && seconds >= 0.).then_some(seconds)
    });
    Some(MoveAnnotation {
        eval_cp: Some(score),
        mate_distance,
        depth: Some(depth),
        elapsed,
        ..Default::default()
    })
}

/// Parse a move comment, normalizing Cute Chess's mover-relative score to White.
/// `white_moved` describes the move carrying the comment, not the next turn.
pub fn parse_move_annotation(comment: &str, white_moved: bool) -> MoveAnnotation {
    let mut note = parse_annotation(comment);
    if !white_moved
        && command(comment, "eval").is_none()
        && field(comment, "wv").is_none()
        && cutechess(comment).is_some()
    {
        note.eval_cp = note.eval_cp.map(|cp| -cp);
        note.mate_distance = note.mate_distance.map(|mate| -mate);
    }
    note
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
        mate_distance: command(comment, "eval").and_then(parse_mate_distance),
        ..Default::default()
    };
    out.depth = field(comment, "d")
        .or_else(|| {
            command(comment, "eval")?
                .split_once(',')
                .map(|(_, depth)| depth)
        })
        .and_then(|value| value.trim().parse().ok());
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
        out.mate_distance = field(comment, "wv").and_then(parse_mate_distance);
    }
    // TCEC gives speed as "s=12345" in kN/s.
    out.nps = field(comment, "s")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|knps| (knps * 1000.0) as u64);
    if let Some(cute) = cutechess(comment) {
        if out.eval_cp.is_none() {
            out.eval_cp = cute.eval_cp;
            out.mate_distance = cute.mate_distance;
        }
        out.depth = out.depth.or(cute.depth);
        out.elapsed = out.elapsed.or(cute.elapsed);
    }
    out
}

/// The `[%eval]` command for a White-relative evaluation: `[%eval 0.35,24]` with
/// the search depth, `[%eval #-3]` for a mate in that many moves. A mate whose
/// distance is not known (`cp` at ±`MATE_CP` without `mate`) cannot be written.
pub fn eval_command(cp: i32, mate: Option<i32>, depth: Option<u32>) -> Option<String> {
    let value = match mate {
        Some(mate) => format!("#{mate}"),
        None if cp.abs() >= MATE_CP => return None,
        None => format!("{:.2}", f64::from(cp) / 100.0),
    };
    Some(match depth.filter(|depth| *depth > 0) {
        Some(depth) => format!("[%eval {value},{depth}]"),
        None => format!("[%eval {value}]"),
    })
}

/// `comment` with its `[%eval]` command replaced by `command`, or with `command`
/// added in front when it has none.
pub fn with_eval_command(comment: &str, command: &str) -> String {
    if let Some(start) = comment.find("[%eval")
        && let Some(end) = comment[start..].find(']')
    {
        return format!(
            "{}{command}{}",
            &comment[..start],
            &comment[start + end + 1..]
        );
    }
    if comment.trim().is_empty() {
        command.to_owned()
    } else {
        format!("{command} {}", comment.trim_start())
    }
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

/// Replaces the readable comment while preserving clock, evaluation and other PGN commands.
pub fn with_comment_text(comment: &str, text: &str) -> String {
    let mut result = text.trim().to_owned();
    let mut rest = comment;
    while let Some(start) = rest.find("[%") {
        let Some(end) = rest[start..].find(']') else {
            break;
        };
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&rest[start..=start + end]);
        rest = &rest[start + end + 1..];
    }
    result
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

    /// Editing prose keeps imported timing and evaluations through a PGN round trip.
    #[test]
    fn editing_comment_preserves_commands() {
        let original = "Old note [%clk 0:00:07] [%emt 0:00:03] [%eval 0.35]";
        let updated = with_comment_text(original, "A better plan");
        assert_eq!(strip_commands(&updated), "A better plan");
        assert_eq!(parse_annotation(&updated), parse_annotation(original));
        assert_eq!(
            with_comment_text(original, ""),
            "[%clk 0:00:07] [%emt 0:00:03] [%eval 0.35]"
        );
        let tree = parse_pgn(&format!("1. e4 {{{updated}}} *")).unwrap();
        let exported = crate::write_pgn(&tree[0]);
        assert!(exported.contains(&updated));
    }

    /// Mate labels retain distance, sign and the source format's score perspective.
    #[test]
    fn mate_distances_survive_import() {
        assert_eq!(parse_annotation("[%eval #5,24]").mate_distance, Some(5));
        assert_eq!(parse_annotation("[%eval #-3]").mate_distance, Some(-3));
        assert_eq!(parse_annotation("[%eval #0]").mate_distance, Some(0));
        assert_eq!(parse_annotation("wv=#-7, d=20").mate_distance, Some(-7));
        assert_eq!(
            parse_move_annotation("-M3/18 0.012s", false).mate_distance,
            Some(3)
        );
        assert_eq!(
            parse_move_annotation("+M2/20 0s", true).mate_distance,
            Some(2)
        );
        assert_eq!(parse_annotation("[%eval 1.25]").mate_distance, None);
        assert_eq!(parse_annotation("[%eval #bad]").mate_distance, None);
        assert_eq!(
            parse_annotation("[%eval 0.80] +M2/20 0s").mate_distance,
            None
        );
    }

    #[test]
    fn eval_commands_round_trip_and_replace_the_old_one() {
        assert_eq!(
            eval_command(35, None, Some(24)).as_deref(),
            Some("[%eval 0.35,24]")
        );
        assert_eq!(
            eval_command(-120, None, None).as_deref(),
            Some("[%eval -1.20]")
        );
        assert_eq!(
            eval_command(-MATE_CP, Some(-3), Some(30)).as_deref(),
            Some("[%eval #-3,30]")
        );
        assert_eq!(eval_command(MATE_CP, None, Some(30)), None);
        let note = parse_annotation("[%eval 0.35,24]");
        assert_eq!((note.eval_cp, note.depth), (Some(35), Some(24)));
        assert_eq!(parse_annotation("[%eval #-3,30]").eval_cp, Some(-MATE_CP));
        assert_eq!(
            with_eval_command("Good [%eval 0.10] [%clk 0:01:00]", "[%eval 0.35,24]"),
            "Good [%eval 0.35,24] [%clk 0:01:00]"
        );
        assert_eq!(with_eval_command("", "[%eval 0.35]"), "[%eval 0.35]");
        assert_eq!(
            with_eval_command("Sharp", "[%eval 0.35]"),
            "[%eval 0.35] Sharp"
        );
    }

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
    fn cutechess_scores_depth_times_and_perspective() {
        let note = parse_move_annotation("+0.35/24 1.25s", false);
        assert_eq!(note.eval_cp, Some(-35));
        assert_eq!(note.depth, Some(24));
        assert_eq!(note.elapsed, Some(1.25));
        assert_eq!(
            parse_move_annotation("-M3/18 0.012s", false).eval_cp,
            Some(MATE_CP)
        );
        assert_eq!(
            parse_move_annotation("+M2/20 0s", true).eval_cp,
            Some(MATE_CP)
        );
        assert_eq!(parse_annotation("-0.42/12 2.4").elapsed, Some(2.4));
        assert_eq!(parse_annotation("book"), MoveAnnotation::default());
        assert_eq!(
            parse_annotation("ordinary/words comment"),
            MoveAnnotation::default()
        );
        assert_eq!(
            parse_move_annotation("+0.35/24 1s [%eval 0.80]", false).eval_cp,
            Some(80)
        );
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
