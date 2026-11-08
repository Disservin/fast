//! Extended Position Description: test suites such as WAC or the Bratko-Kopec set,
//! one position per line with operations like `bm` (best move), `am` (avoid move),
//! `dm` (direct mate in N) and `id`.

use shakmaty::{
    Chess, EnPassantMode, Move, PositionError,
    fen::Fen,
    san::{San, SanPlus},
    uci::UciMove,
};

/// One parsed EPD record.
#[derive(Clone, Debug)]
pub struct EpdPosition {
    /// The position as a full FEN, with the move counters from `hmvc`/`fmvn` or the
    /// line itself when present.
    pub fen: String,
    pub pos: Chess,
    pub id: Option<String>,
    /// `bm`: any of these solves the position.
    pub best: Vec<Move>,
    /// `am`: none of these may be played.
    pub avoid: Vec<Move>,
    /// `dm`: the side to move mates in this many moves.
    pub mate: Option<u32>,
    /// `c0`, the usual comment.
    pub comment: Option<String>,
}

/// A line of the file and what came of parsing it.
#[derive(Clone, Debug)]
pub struct EpdLine {
    /// 1-based line number in the file.
    pub line: usize,
    pub text: String,
    pub parsed: Result<EpdPosition, String>,
}

impl EpdPosition {
    /// The move in SAN for this position, e.g. "Qxf7+".
    pub fn san(&self, mv: &Move) -> String {
        SanPlus::from_move(self.pos.clone(), *mv).to_string()
    }

    /// The engine's UCI move as a move in this position, if it is legal.
    pub fn uci_move(&self, uci: &str) -> Option<Move> {
        uci.parse::<UciMove>().ok()?.to_move(&self.pos).ok()
    }

    /// Whether playing `mv` meets the `bm`/`am` operations. `None` when the record
    /// has neither.
    pub fn move_solves(&self, mv: &Move) -> Option<bool> {
        if self.best.is_empty() && self.avoid.is_empty() {
            return None;
        }
        Some((self.best.is_empty() || self.best.contains(mv)) && !self.avoid.contains(mv))
    }

    /// "bm Qxf7+ Nf6", "am Bxh7", "dm 3": what the record expects.
    pub fn expectation(&self) -> String {
        let mut parts = Vec::new();
        let list = |moves: &[Move]| {
            moves
                .iter()
                .map(|mv| self.san(mv))
                .collect::<Vec<_>>()
                .join(" ")
        };
        if !self.best.is_empty() {
            parts.push(format!("bm {}", list(&self.best)));
        }
        if !self.avoid.is_empty() {
            parts.push(format!("am {}", list(&self.avoid)));
        }
        if let Some(mate) = self.mate {
            parts.push(format!("dm {mate}"));
        }
        parts.join(", ")
    }
}

/// Parses every non-empty line that is not a `#` or `%` comment. Lines that fail keep
/// their reason, so a suite with a few broken records can still run the rest.
pub fn parse_epd(text: &str) -> Vec<EpdLine> {
    text.lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            // Some suites end lines with stray control characters such as NUL.
            let trimmed = raw
                .trim_matches(|ch: char| ch.is_whitespace() || ch.is_control())
                .trim_start_matches('\u{feff}');
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('%') {
                return None;
            }
            Some(EpdLine {
                line: index + 1,
                text: trimmed.to_owned(),
                parsed: parse_record(trimmed),
            })
        })
        .collect()
}

/// Parses one EPD record.
pub fn parse_record(line: &str) -> Result<EpdPosition, String> {
    let line: String = line
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let mut rest = line.trim();
    let mut fields = Vec::with_capacity(4);
    for _ in 0..4 {
        let (field, tail) = split_field(rest);
        if field.is_empty() {
            return Err("needs board, side to move, castling and en passant fields".into());
        }
        fields.push(field);
        rest = tail;
    }
    // Some suites are full FENs followed by operations: take the two counters too.
    let mut halfmove = None;
    let mut fullmove = None;
    let (first, after_first) = split_field(rest);
    if let Ok(value) = first.parse::<u32>() {
        halfmove = Some(value);
        rest = after_first;
        let (second, after_second) = split_field(rest);
        if let Ok(value) = second.parse::<u32>() {
            fullmove = Some(value);
            rest = after_second;
        }
    }

    let mut operations = Vec::new();
    for op in split_operations(rest)? {
        let (opcode, operands) = split_token(&op);
        if opcode.is_empty() {
            continue;
        }
        operations.push((opcode.to_owned(), operands_of(operands)?));
    }
    for (opcode, operands) in &operations {
        let number = || operands.first().and_then(|value| value.parse::<u32>().ok());
        match opcode.as_str() {
            "hmvc" => halfmove = number().or(halfmove),
            "fmvn" => fullmove = number().or(fullmove),
            _ => {}
        }
    }

    let fen = format!(
        "{} {} {} {} {} {}",
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        halfmove.unwrap_or(0),
        fullmove.unwrap_or(1).max(1)
    );
    let parsed: Fen = fen.parse().map_err(|err| format!("invalid FEN: {err}"))?;
    // EPD records an en passant square after every double push, and some suites carry one
    // that no pawn could have made. It only matters for an en passant capture, so a square
    // that cannot be right is dropped rather than rejecting the position. Composed studies
    // and fortresses may have more material or checks than a game could reach; engines
    // search them all the same.
    let pos: Chess = parsed
        .into_position(crate::fen_variant(&fen).unwrap_or_default().castling_mode())
        .or_else(PositionError::ignore_invalid_ep_square)
        .or_else(PositionError::ignore_too_much_material)
        .or_else(PositionError::ignore_impossible_check)
        .map_err(|err| format!("illegal position: {err}"))?;
    let fen = Fen::from_position(&pos, EnPassantMode::Legal).to_string();

    let mut record = EpdPosition {
        fen,
        pos,
        id: None,
        best: Vec::new(),
        avoid: Vec::new(),
        mate: None,
        comment: None,
    };
    for (opcode, operands) in operations {
        match opcode.as_str() {
            "bm" | "am" => {
                let mut moves = Vec::with_capacity(operands.len());
                let mut mate_score = false;
                for text in &join_promotions(&operands) {
                    // Annotations written apart from the move, as in "am Nxb6 + ?".
                    if text.chars().all(|ch| matches!(ch, '+' | '#' | '!' | '?')) {
                        continue;
                    }
                    // Mate suites such as matetrack write "bm #3" for mate in 3 and "bm #-3"
                    // for being mated in 3.
                    if opcode == "bm"
                        && let Some(moves) = text
                            .strip_prefix('#')
                            .and_then(|value| value.parse::<i32>().ok())
                    {
                        mate_score = true;
                        if moves > 0 && record.mate.is_none() {
                            record.mate = Some(moves.unsigned_abs());
                        }
                        continue;
                    }
                    let mv = parse_move(&record.pos, text)
                        .ok_or_else(|| format!("{opcode} {text} is not a legal move here"))?;
                    moves.push(mv);
                }
                if moves.is_empty() && !mate_score {
                    return Err(format!("{opcode} has no moves"));
                }
                if opcode == "bm" {
                    record.best = moves;
                } else {
                    record.avoid = moves;
                }
            }
            "dm" => {
                let mate = operands
                    .first()
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|mate| *mate > 0)
                    .ok_or_else(|| "dm needs a positive number of moves".to_owned())?;
                record.mate = Some(mate);
            }
            "id" => record.id = operands.first().cloned(),
            "c0" => record.comment = Some(operands.join(" ")),
            _ => {}
        }
    }
    Ok(record)
}

/// A SAN move such as "Qxf7+" or "Nf3!", or a UCI move such as "e2e4".
fn parse_move(pos: &Chess, text: &str) -> Option<Move> {
    let cleaned = text.trim_end_matches(['!', '?']);
    // "c8N" for "c8=N": a promotion written without its equals sign.
    let spelled;
    let cleaned = match cleaned
        .char_indices()
        .rev()
        .find(|(_, ch)| !matches!(ch, '+' | '#'))
    {
        Some((at, 'Q' | 'R' | 'B' | 'N'))
            if at > 0 && matches!(cleaned.as_bytes()[at - 1], b'1' | b'8') =>
        {
            spelled = format!("{}={}", &cleaned[..at], &cleaned[at..]);
            spelled.as_str()
        }
        _ => cleaned,
    };
    if let Ok(san) = cleaned.parse::<SanPlus>()
        && let Ok(mv) = san.san.to_move(pos)
    {
        return Some(mv);
    }
    if let Ok(san) = cleaned.trim_end_matches(['+', '#']).parse::<San>()
        && let Ok(mv) = san.to_move(pos)
    {
        return Some(mv);
    }
    cleaned.parse::<UciMove>().ok()?.to_move(pos).ok()
}

/// Joins a promotion written with spaces, "f8 = Q", back into one move.
fn join_promotions(operands: &[String]) -> Vec<String> {
    let mut joined: Vec<String> = Vec::with_capacity(operands.len());
    let mut iter = operands.iter();
    while let Some(text) = iter.next() {
        if text == "=" && !joined.is_empty() {
            let piece = iter.next().map_or("", String::as_str);
            if let Some(last) = joined.last_mut() {
                last.push('=');
                last.push_str(piece);
            }
        } else {
            joined.push(text.clone());
        }
    }
    joined
}

/// The next position field or move counter. A `;` ends it too, since some suites write
/// "w - -; bm e4" with no space before the first operation.
fn split_field(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(|ch: char| ch.is_whitespace() || ch == ';') {
        Some(end) => (&text[..end], &text[end..]),
        None => (text, ""),
    }
}

fn split_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(end) => (&text[..end], &text[end..]),
        None => (text, ""),
    }
}

/// Splits on `;` outside quoted strings. A final operation may omit its `;`.
fn split_operations(text: &str) -> Result<Vec<String>, String> {
    let mut operations = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut previous = ' ';
    for ch in text.chars() {
        let starts_word = previous.is_whitespace() || previous == ';';
        previous = ch;
        match ch {
            // A quote opens a string only at the start of a word, so the inch mark in a
            // note such as 00'40" stays plain text.
            '"' if quoted || starts_word => {
                quoted = !quoted;
                current.push(ch);
            }
            ';' if !quoted => operations.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if quoted {
        return Err("unterminated quoted string".into());
    }
    if !current.trim().is_empty() {
        operations.push(current);
    }
    Ok(operations)
}

/// Operands separated by spaces, with quoted strings kept whole and unquoted.
fn operands_of(text: &str) -> Result<Vec<String>, String> {
    let mut operands = Vec::new();
    let mut chars = text.trim().chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else if ch == '"' {
            chars.next();
            let mut value = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(ch) => value.push(ch),
                    None => return Err("unterminated quoted string".into()),
                }
            }
            operands.push(value);
        } else {
            let mut value = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                value.push(ch);
                chars.next();
            }
            operands.push(value);
        }
    }
    Ok(operands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_best_moves_ids_and_comments() {
        let line = r#"2rr3k/pp3pp1/1nnqbN1p/3pN3/2pP4/2P3Q1/PPB4P/R4RK1 w - - bm Qg6; id "WAC.001"; c0 "a; b";"#;
        let record = parse_record(line).unwrap();
        assert_eq!(record.id.as_deref(), Some("WAC.001"));
        assert_eq!(record.comment.as_deref(), Some("a; b"));
        assert_eq!(record.best.len(), 1);
        assert_eq!(record.san(&record.best[0]), "Qg6");
        assert_eq!(
            record.fen,
            "2rr3k/pp3pp1/1nnqbN1p/3pN3/2pP4/2P3Q1/PPB4P/R4RK1 w - - 0 1"
        );
        let qg6 = record.uci_move("g3g6").unwrap();
        assert_eq!(record.move_solves(&qg6), Some(true));
        let other = record.uci_move("a2a3").unwrap();
        assert_eq!(record.move_solves(&other), Some(false));
    }

    #[test]
    fn accepts_counters_mates_avoid_moves_and_uci() {
        let record = parse_record("4k3/8/4K3/8/8/8/8/R7 w - - 0 1 dm 1; am Ra2;").unwrap();
        assert_eq!(record.mate, Some(1));
        let ra2 = record.uci_move("a1a2").unwrap();
        assert_eq!(record.move_solves(&ra2), Some(false));
        let ra8 = record.uci_move("a1a8").unwrap();
        assert_eq!(record.move_solves(&ra8), Some(true));
        assert_eq!(record.expectation(), "am Ra2, dm 1");

        let uci =
            parse_record("4k3/8/4K3/8/8/8/8/R7 w - - bm a1a8 Ra8#; hmvc 3; fmvn 40;").unwrap();
        assert_eq!(uci.best.len(), 2);
        assert_eq!(uci.fen, "4k3/8/4K3/8/8/8/8/R7 w - - 3 40");
    }

    #[test]
    fn reads_loosely_written_suites() {
        // The first operation right after the en passant field, spaced-out annotations and a
        // trailing NUL, as in the fortress suites.
        let glued = parse_epd(
            "2k5/2p5/1q1p4/pPpPp1p1/N1P1Pp1p/P4PbP/KQ4P1/8 w - -; am Nxb6 + ? ; c1 diff=10\0\n",
        );
        let record = glued[0].parsed.as_ref().unwrap();
        assert_eq!(record.avoid.len(), 1);
        assert_eq!(record.san(&record.avoid[0]), "Nxb6+");
        assert!(!glued[0].text.contains('\0'));

        let counters = parse_record("4k3/8/4K3/8/8/8/8/R7 w - - 5 30; bm Ra8;").unwrap();
        assert_eq!(counters.fen, "4k3/8/4K3/8/8/8/8/R7 w - - 5 30");

        // An en passant square no double push could have made is dropped.
        let ep = parse_record("4k3/8/8/8/8/8/8/R3K3 w - e6 bm Ra8;").unwrap();
        assert_eq!(ep.fen, "4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
        // A real one is kept for the capture.
        let real = parse_record("5K2/8/2qk4/2nPp3/3r4/6B1/B7/3R4 w - e6 bm dxe6#;").unwrap();
        assert_eq!(real.san(&real.best[0]), "dxe6#");
    }

    #[test]
    fn reads_mate_scores_written_as_best_moves() {
        let mate = parse_record("5K2/8/2qk4/2nPp3/3r4/6B1/B7/3R4 w - e6 bm #1; PV: d5e6;").unwrap();
        assert_eq!(mate.mate, Some(1));
        assert!(mate.best.is_empty());
        let mated =
            parse_record("6Qk/PP1PP1np/1N1BN2P/7R/4B3/2Q5/P3KP2/6R1 b - - bm #-1;").unwrap();
        assert_eq!(mated.mate, None);
        let note = parse_record(
            "rnb1k2r/pppp3p/6q1/7Q/8/8/PP2BKPP/R1B4R w kq - bm #11; 00'40\" (Gustav); c0 \"a; b\";",
        )
        .unwrap();
        assert_eq!(note.mate, Some(11));
        assert_eq!(note.comment.as_deref(), Some("a; b"));
    }

    #[test]
    fn reads_promotions_written_loosely() {
        let plain = parse_record("5nr1/2Pp2pk/3Pp1p1/4P1P1/6P1/5K2/8/7n w - - ; bm c8N").unwrap();
        assert_eq!(plain.san(&plain.best[0]), "c8=N");
        let spaced =
            parse_record("8/2pPpP2/1P1qk1p1/1p4P1/1P4p1/1p1QK1P1/2PpPp2/8 w - -; bm f8 = Q;")
                .unwrap();
        assert!(spaced.san(&spaced.best[0]).starts_with("f8=Q"));
    }

    #[test]
    fn accepts_composed_positions() {
        let study = parse_record(
            "k7/1p1p1p1p/pPpPpPpP/P1P1P1P1/1B1B1B1B/B1B1B1B1/1B1B1B1B/K1B1B1B1 w - - 0 1; draw",
        );
        assert!(study.is_ok());
    }

    #[test]
    fn reports_broken_lines_and_skips_comments() {
        let text = "\
# a comment
4k3/8/4K3/8/8/8/8/R7 w - - bm Ra8;

not a fen at all
4k3/8/4K3/8/8/8/8/R7 w - - bm Qh5;
4k3/8/4K3/8/8/8/8/R7 w - - dm 0;
8/8/8/8/8/8/8/8 w - - bm e4;
4k3/8/4K3/8/8/8/8/R7 w - - id \"open;
";
        let lines = parse_epd(text);
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0].line, 2);
        assert!(lines[0].parsed.is_ok());
        for broken in &lines[1..] {
            assert!(broken.parsed.is_err(), "line {} should fail", broken.line);
        }
        assert!(
            lines[2]
                .parsed
                .as_ref()
                .unwrap_err()
                .contains("not a legal move")
        );
    }
}
