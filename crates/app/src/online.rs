//! Optional online lookups for the Analyse screen: the ChessTB Explorer endgame tablebase
//! and the chessdb.cn cloud database. Nothing here runs unless the Online panel is open.
//!
//! Requests block, so callers run them on a background task.

use std::{sync::OnceLock, time::Duration};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

const CHESSTB_URL: &str = "https://chesstb-explorer.hf.space/probe";
const CHESSDB_URL: &str = "https://www.chessdb.cn/cdb.php";
/// ChessTB Explorer rejects longer FENs.
const MAX_FEN: usize = 100;
/// Positions with more pieces than this are never sent to the tablebase.
pub const TABLEBASE_PIECES: usize = 8;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .https_only(true)
            .http_status_as_error(false)
            .user_agent(concat!("Fast/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

/// A game result from the side to move's point of view.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Win,
    CursedWin,
    Draw,
    BlessedLoss,
    Loss,
    Unknown,
    NotAvailable,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Win => "Win",
            Outcome::CursedWin => "Cursed win",
            Outcome::Draw => "Draw",
            Outcome::BlessedLoss => "Blessed loss",
            Outcome::Loss => "Loss",
            Outcome::Unknown => "Unknown",
            Outcome::NotAvailable => "n/a",
        }
    }

    fn from_wdl(wdl: i32) -> Self {
        match wdl {
            2 => Outcome::Win,
            1 => Outcome::CursedWin,
            0 => Outcome::Draw,
            -1 => Outcome::BlessedLoss,
            -2 => Outcome::Loss,
            _ => Outcome::Unknown,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TbMove {
    pub san: String,
    #[serde(default)]
    pub plies: i32,
    pub outcome: Outcome,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub is_mate: bool,
    #[serde(default)]
    pub draw_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TbProbe {
    pub wdl: i32,
    pub dtz: Option<i32>,
    pub dtm: Option<i32>,
    #[serde(default)]
    pub draw_reason: Option<String>,
    #[serde(default)]
    pub moves_dtz: Vec<TbMove>,
    #[serde(default)]
    pub moves_dtm: Vec<TbMove>,
}

/// One row of the tablebase table: a move with both of its distances.
#[derive(Clone, Debug, PartialEq)]
pub struct TbRow {
    pub san: String,
    /// From the side to move's point of view, as the move leaves it.
    pub outcome: Outcome,
    pub dtz: Option<i32>,
    pub dtm: Option<i32>,
    pub is_mate: bool,
    pub draw_reason: Option<String>,
}

impl TbProbe {
    pub fn outcome(&self) -> Outcome {
        Outcome::from_wdl(self.wdl)
    }

    /// Moves in DTM order when DTM is known for this material, otherwise DTZ order,
    /// with the other distance alongside.
    pub fn rows(&self) -> Vec<TbRow> {
        let dtm_known = self.moves_dtm.iter().any(|mv| mv.available);
        let (primary, other) = if dtm_known {
            (&self.moves_dtm, &self.moves_dtz)
        } else {
            (&self.moves_dtz, &self.moves_dtm)
        };
        let distance = |mv: &TbMove| mv.available.then_some(mv.plies);
        primary
            .iter()
            .map(|mv| {
                let paired = other.iter().find(|o| o.san == mv.san);
                let (dtz, dtm) = if dtm_known {
                    (paired.and_then(distance), distance(mv))
                } else {
                    (distance(mv), paired.and_then(distance))
                };
                let outcome = if mv.outcome == Outcome::NotAvailable {
                    paired.map_or(mv.outcome, |o| o.outcome)
                } else {
                    mv.outcome
                };
                TbRow {
                    san: mv.san.clone(),
                    outcome,
                    dtz,
                    dtm,
                    is_mate: mv.is_mate,
                    draw_reason: mv.draw_reason.clone(),
                }
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct TbError {
    error: Option<String>,
    error_code: Option<String>,
}

/// Probes ChessTB Explorer. `Ok(None)` means the position is outside its tablebase.
pub fn probe_tablebase(fen: &str) -> Result<Option<TbProbe>> {
    if fen.len() > MAX_FEN {
        bail!("FEN is too long for the tablebase");
    }
    let body = serde_json::json!({ "fen": fen }).to_string();
    let mut response = agent()
        .post(CHESSTB_URL)
        .header("Content-Type", "application/json")
        .send(body.as_str())
        .context("Could not reach ChessTB Explorer")?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .context("Reading the tablebase reply")?;
    parse_tablebase(status, &text)
}

fn parse_tablebase(status: u16, text: &str) -> Result<Option<TbProbe>> {
    if status == 200 {
        return serde_json::from_str(text)
            .map(Some)
            .context("Unexpected tablebase reply");
    }
    let error: Option<TbError> = serde_json::from_str(text).ok();
    if let Some(error) = &error
        && error.error_code.as_deref() == Some("missing_table")
    {
        return Ok(None);
    }
    let message = error.and_then(|error| error.error);
    match status {
        429 => bail!("Tablebase rate limit reached; try again shortly"),
        _ => bail!(
            "Tablebase error {status}{}",
            message.map(|m| format!(": {m}")).unwrap_or_default()
        ),
    }
}

/// A move in the chessdb.cn cloud database.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CloudMove {
    pub san: String,
    /// Centipawns for the side to move; mates and tablebase results are near ±30000.
    pub score: i32,
    /// 2 is a best move, 1 a good one, 0 the rest.
    #[serde(default)]
    pub rank: i32,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub winrate: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CloudReply {
    Moves(Vec<CloudMove>),
    /// The database has no analysis for this position yet.
    Unknown,
    /// The game is over: checkmate or stalemate.
    Over(String),
}

#[derive(Debug, Deserialize)]
struct CloudBody {
    status: String,
    #[serde(default)]
    moves: Vec<CloudMove>,
}

/// Queries every known move of a position from chessdb.cn.
pub fn query_chessdb(fen: &str) -> Result<CloudReply> {
    let mut response = agent()
        .get(CHESSDB_URL)
        .query("action", "queryall")
        .query("json", "1")
        .query("board", fen)
        .call()
        .context("Could not reach chessdb.cn")?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .context("Reading the chessdb.cn reply")?;
    if status != 200 {
        bail!("chessdb.cn error {status}");
    }
    parse_chessdb(&text)
}

fn parse_chessdb(text: &str) -> Result<CloudReply> {
    let body: CloudBody = serde_json::from_str(text).context("Unexpected chessdb.cn reply")?;
    match body.status.as_str() {
        "ok" => Ok(CloudReply::Moves(body.moves)),
        "unknown" => Ok(CloudReply::Unknown),
        "checkmate" => Ok(CloudReply::Over("Checkmate".into())),
        "stalemate" => Ok(CloudReply::Over("Stalemate".into())),
        "rate limited" => bail!("chessdb.cn rate limit reached; try again shortly"),
        other => bail!("chessdb.cn: {other}"),
    }
}

/// chessdb.cn's score as text: mates as `#n`, tablebase wins as `TB`, else pawns.
pub fn cloud_score(score: i32) -> String {
    const MATE: i32 = 30000;
    const TB_WIN: i32 = 25000;
    if score.abs() > MATE - 1000 {
        let plies = MATE - score.abs();
        let moves = (plies + 1) / 2;
        return format!("{}#{moves}", if score < 0 { "-" } else { "" });
    }
    if score.abs() > TB_WIN - 1000 {
        return if score < 0 { "-TB" } else { "+TB" }.into();
    }
    format!("{:+.2}", score as f64 / 100.)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KRK: &str = r#"{"wdl": 2, "dtz": 21, "dtz_available": true, "dtm": 21,
        "dtm_available": true, "draw_reason": null,
        "moves_dtz": [
            {"san": "Rh7", "plies": 21, "outcome": "win", "available": true, "is_mate": false},
            {"san": "Kd2", "plies": 25, "outcome": "win", "available": true, "is_mate": false}],
        "moves_dtm": [
            {"san": "Kd2", "plies": 19, "outcome": "win", "available": true, "is_mate": false},
            {"san": "Rh7", "plies": 21, "outcome": "win", "available": true, "is_mate": false}],
        "summary": {"wins": 2, "draws": 0, "losses": 0, "unknown": 0}}"#;

    #[test]
    fn tablebase_rows_follow_dtm_and_pair_dtz() {
        let probe = parse_tablebase(200, KRK).unwrap().unwrap();
        assert_eq!(probe.outcome(), Outcome::Win);
        let rows = probe.rows();
        assert_eq!(rows[0].san, "Kd2");
        assert_eq!((rows[0].dtm, rows[0].dtz), (Some(19), Some(25)));
        assert_eq!((rows[1].dtm, rows[1].dtz), (Some(21), Some(21)));
    }

    #[test]
    fn tablebase_falls_back_to_dtz_without_dtm() {
        let text = r#"{"wdl": 0, "dtz": 0, "dtm": null,
            "moves_dtz": [{"san": "Kd1", "plies": 0, "outcome": "draw", "available": true}],
            "moves_dtm": [{"san": "Kd1", "plies": 0, "outcome": "not_available", "available": false}]}"#;
        let rows = parse_tablebase(200, text).unwrap().unwrap().rows();
        assert_eq!(rows[0].outcome, Outcome::Draw);
        assert_eq!((rows[0].dtz, rows[0].dtm), (Some(0), None));
    }

    #[test]
    fn missing_table_is_not_an_error() {
        let text = r#"{"error": "Position not covered", "error_code": "missing_table", "piece_count": 32}"#;
        assert!(parse_tablebase(400, text).unwrap().is_none());
        assert!(parse_tablebase(429, r#"{"error": "slow down"}"#).is_err());
    }

    #[test]
    fn chessdb_statuses() {
        let ok = r#"{"status":"ok","moves":[{"uci":"e7e5","san":"e5","score":0,"rank":2,
            "note":"! (29-15)","winrate":"50.00"}]}"#;
        let CloudReply::Moves(moves) = parse_chessdb(ok).unwrap() else {
            panic!("expected moves")
        };
        assert_eq!(moves[0].san, "e5");
        assert_eq!(moves[0].rank, 2);
        assert_eq!(
            parse_chessdb(r#"{"status":"unknown"}"#).unwrap(),
            CloudReply::Unknown
        );
        assert!(matches!(
            parse_chessdb(r#"{"status":"checkmate"}"#).unwrap(),
            CloudReply::Over(_)
        ));
        assert!(parse_chessdb(r#"{"status":"invalid board"}"#).is_err());
    }

    #[test]
    fn cloud_scores_read_as_pawns_mates_and_tablebase() {
        assert_eq!(cloud_score(35), "+0.35");
        assert_eq!(cloud_score(-120), "-1.20");
        assert_eq!(cloud_score(29995), "#3");
        assert_eq!(cloud_score(-29996), "-#2");
        assert_eq!(cloud_score(24990), "+TB");
    }

    /// Calls the real services: `cargo test -p fast online -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn live_services_answer() {
        let probe = probe_tablebase("4k3/8/8/8/8/8/8/4K2R w - - 0 1")
            .unwrap()
            .unwrap();
        assert_eq!(probe.outcome(), Outcome::Win);
        assert!(!probe.rows().is_empty());
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert!(probe_tablebase(start).unwrap().is_none());
        assert!(matches!(query_chessdb(start).unwrap(), CloudReply::Moves(m) if !m.is_empty()));
    }
}
