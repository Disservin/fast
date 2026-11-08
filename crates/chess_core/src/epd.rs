//! Extended Position Description: test suites such as WAC or the Bratko-Kopec set,
//! one position per line with operations like `bm` (best move), `am` (avoid move),
//! `dm` (direct mate in N) and `id`.

use shakmaty::{
    CastlingMode, Chess, EnPassantMode, Move,
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
            let trimmed = raw.trim().trim_start_matches('\u{feff}');
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
    let mut rest = line.trim();
    let mut fields = Vec::with_capacity(4);
    for _ in 0..4 {
        let (field, tail) = split_token(rest);
        if field.is_empty() {
            return Err("needs board, side to move, castling and en passant fields".into());
        }
        fields.push(field);
        rest = tail;
    }
    // Some suites are full FENs followed by operations: take the two counters too.
    let mut halfmove = None;
    let mut fullmove = None;
    let (first, after_first) = split_token(rest);
    if let Ok(value) = first.parse::<u32>() {
        halfmove = Some(value);
        rest = after_first;
        let (second, after_second) = split_token(rest);
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
    let pos: Chess = parsed
        .into_position(CastlingMode::Standard)
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
                if operands.is_empty() {
                    return Err(format!("{opcode} has no moves"));
                }
                let mut moves = Vec::with_capacity(operands.len());
                for text in &operands {
                    let mv = parse_move(&record.pos, text)
                        .ok_or_else(|| format!("{opcode} {text} is not a legal move here"))?;
                    moves.push(mv);
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
    for ch in text.chars() {
        match ch {
            '"' => {
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
