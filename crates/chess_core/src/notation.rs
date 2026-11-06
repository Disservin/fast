use serde::{Deserialize, Serialize};
use shakmaty::{Chess, Move, san::SanPlus, uci::UciMove};

/// How moves are displayed. SAN is what is stored; the others are presentation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notation {
    #[default]
    San,
    Figurine,
    Long,
}

/// Text for a move in the chosen notation.
pub fn display_san(san: &str, mv: Option<&Move>, notation: Notation) -> String {
    match notation {
        Notation::San => san.to_owned(),
        Notation::Figurine => {
            let mut out = String::with_capacity(san.len() + 2);
            for ch in san.chars() {
                let figurine = match ch {
                    'K' => '♔',
                    'Q' => '♕',
                    'R' => '♖',
                    'B' => '♗',
                    'N' => '♘',
                    other => other,
                };
                out.push(figurine);
            }
            out
        }
        Notation::Long => match mv {
            Some(Move::Castle { .. }) | None => san.to_owned(),
            Some(mv) => {
                let (Some(from), to) = (mv.from(), mv.to()) else {
                    return san.to_owned();
                };
                let piece = match mv.role() {
                    shakmaty::Role::Pawn => String::new(),
                    role => role.upper_char().to_string(),
                };
                let separator = if mv.is_capture() { 'x' } else { '-' };
                let promotion = mv
                    .promotion()
                    .map(|role| format!("={}", role.upper_char()))
                    .unwrap_or_default();
                format!("{piece}{from}{separator}{to}{promotion}")
            }
        },
    }
}

/// "14.f4" for White, "13…Ne8" for Black.
pub fn numbered(ply: u32, san: &str) -> String {
    let number = ply.div_ceil(2);
    if ply % 2 == 1 {
        format!("{number}.{san}")
    } else {
        format!("{number}…{san}")
    }
}

/// Conventional glyph for the NAGs the UI can show.
pub fn nag_symbol(nag: u8) -> Option<&'static str> {
    Some(match nag {
        1 => "!",
        2 => "?",
        3 => "!!",
        4 => "??",
        5 => "!?",
        6 => "?!",
        10 | 11 | 12 => "=",
        13 => "∞",
        14 => "⩲",
        15 => "⩱",
        16 => "±",
        17 => "∓",
        18 => "+-",
        19 => "-+",
        _ => return None,
    })
}

/// Converts a UCI principal variation into SAN strings, stopping at the first
/// move that is not legal (engines can emit truncated or stale lines).
pub fn pv_to_san(pos: &Chess, pv: &[String]) -> Vec<String> {
    let mut pos = pos.clone();
    let mut out = Vec::with_capacity(pv.len());
    for uci in pv {
        let Ok(parsed) = uci.parse::<UciMove>() else {
            break;
        };
        let Ok(mv) = parsed.to_move(&pos) else {
            break;
        };
        out.push(SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string());
    }
    out
}

/// "14.f4 a5 15.f5" / "13…Ne8 14.f4": a SAN line with move numbers.
pub fn format_pv(start_ply: u32, sans: &[String]) -> String {
    let mut out = String::new();
    for (index, san) in sans.iter().enumerate() {
        let ply = start_ply + index as u32 + 1;
        if index > 0 {
            out.push(' ');
        }
        if ply % 2 == 1 || index == 0 {
            out.push_str(&numbered(ply, san));
        } else {
            out.push_str(san);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering() {
        assert_eq!(numbered(27, "f4"), "14.f4");
        assert_eq!(numbered(26, "Ne8"), "13…Ne8");
    }

    #[test]
    fn pv_stops_at_illegal_moves() {
        let pos = Chess::default();
        let pv = ["e2e4", "e7e5", "e4e5", "g1f3"].map(String::from);
        assert_eq!(pv_to_san(&pos, &pv), vec!["e4", "e5"]);
        assert_eq!(pv_to_san(&pos, &["nonsense".to_owned()]), Vec::<String>::new());
    }

    #[test]
    fn pv_formatting_starts_with_black_ellipsis() {
        let line = ["a5", "f5", "Bc4"].map(String::from);
        assert_eq!(format_pv(27, &line), "14…a5 15.f5 Bc4");
        assert_eq!(format_pv(26, &line), "14.a5 f5 15.Bc4");
    }

    #[test]
    fn notations() {
        let pos = Chess::default();
        let mv = "g1f3".parse::<UciMove>().unwrap().to_move(&pos).unwrap();
        assert_eq!(display_san("Nf3", Some(&mv), Notation::San), "Nf3");
        assert_eq!(display_san("Nf3", Some(&mv), Notation::Figurine), "♘f3");
        assert_eq!(display_san("Nf3", Some(&mv), Notation::Long), "Ng1-f3");
    }
}
