use rusqlite::{OptionalExtension, Row, ToSql, params, params_from_iter};

use crate::{Database, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeClass {
    Bullet,
    Blitz,
    Rapid,
    Classical,
    Unknown,
}

impl TimeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bullet => "bullet",
            Self::Blitz => "blitz",
            Self::Rapid => "rapid",
            Self::Classical => "classical",
            Self::Unknown => "unknown",
        }
    }

    /// Classifies a PGN TimeControl tag ("600+5") by estimated 40-move duration.
    pub fn from_time_control(control: &str) -> Self {
        let mut parts = control.split('+');
        let Some(Ok(base)) = parts.next().map(str::parse::<u32>) else {
            return Self::Unknown;
        };
        let increment = parts
            .next()
            .and_then(|inc| inc.parse::<u32>().ok())
            .unwrap_or(0);
        match base + 40 * increment {
            0..180 => Self::Bullet,
            180..480 => Self::Blitz,
            480..1500 => Self::Rapid,
            _ => Self::Classical,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideFilter {
    #[default]
    Any,
    White,
    Black,
}

/// A game result to filter by. With a player set it is that player's result;
/// otherwise Win is 1-0 and Loss is 0-1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultFilter {
    Win,
    Draw,
    Loss,
}

/// Reads a date bound typed as `2020`, `2020.05` or `2020.05.17` (dots or dashes) and
/// pads it to the stored `YYYY-MM-DD` form. A lower bound fills missing parts with the
/// smallest value; an upper bound with one above any digit or `?`, so `2020` up to
/// includes `2020-??-??`. Returns `None` for anything else.
pub fn date_bound(text: &str, upper: bool) -> Option<String> {
    let text = text.trim();
    let parts: Vec<&str> = text.split(['.', '-', '/']).collect();
    let valid = |part: &str, len: usize, range: std::ops::RangeInclusive<u32>| {
        part.len() == len
            && part
                .parse::<u32>()
                .is_ok_and(|value| range.contains(&value))
    };
    if parts.is_empty()
        || parts.len() > 3
        || !valid(parts[0], 4, 1000..=9999)
        || parts.get(1).is_some_and(|month| !valid(month, 2, 1..=12))
        || parts.get(2).is_some_and(|day| !valid(day, 2, 1..=31))
    {
        return None;
    }
    let fill = if upper { "~~" } else { "00" };
    let month = parts.get(1).copied().unwrap_or(fill);
    let day = parts.get(2).copied().unwrap_or(fill);
    Some(format!("{}-{month}-{day}", parts[0]))
}

/// Escapes `%`, `_` and `\` for a LIKE pattern that finds `text` anywhere.
fn contains_pattern(text: &str) -> String {
    format!(
        "%{}%",
        text.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

/// A column the games table can be sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortKey {
    /// The order the games were imported in, which is their order in the file.
    FileOrder,
    White,
    Black,
    Result,
    Moves,
    #[default]
    Date,
    Source,
    Accuracy,
}

impl SortKey {
    pub const ALL: [Self; 8] = [
        Self::FileOrder,
        Self::White,
        Self::Black,
        Self::Result,
        Self::Moves,
        Self::Date,
        Self::Source,
        Self::Accuracy,
    ];

    /// The fixed SQL expression for this key, so user input never reaches the query.
    fn sql(self) -> &'static str {
        match self {
            Self::FileOrder => "id",
            Self::White => "white COLLATE NOCASE",
            Self::Black => "black COLLATE NOCASE",
            Self::Result => "result",
            Self::Moves => "plies",
            Self::Date => "date",
            Self::Source => "source COLLATE NOCASE",
            Self::Accuracy => "accuracy",
        }
    }

    /// A stable name for saving the key in the config.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileOrder => "file",
            Self::White => "white",
            Self::Black => "black",
            Self::Result => "result",
            Self::Moves => "moves",
            Self::Date => "date",
            Self::Source => "source",
            Self::Accuracy => "accuracy",
        }
    }

    /// The key saved under `name`, if there is one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == name)
    }
}

/// The games table's order: a column and its direction. Ties keep file order in the
/// same direction, so the order is stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    /// Newest first.
    fn default() -> Self {
        Self {
            key: SortKey::Date,
            descending: true,
        }
    }
}

impl Sort {
    /// File order, first game first: the default for a database imported from one file.
    pub const FILE: Self = Self {
        key: SortKey::FileOrder,
        descending: false,
    };

    /// The ORDER BY clause. With `filtered`, the sort column is written as `+column`
    /// so SQLite scans the table and sorts the matches, which is several times faster
    /// than walking the sort index and fetching every row to test it.
    fn order_by(self, filtered: bool) -> String {
        let direction = if self.descending { "DESC" } else { "ASC" };
        let plus = if filtered { "+" } else { "" };
        match self.key {
            SortKey::FileOrder => format!("id {direction}"),
            key => format!("{plus}{} {direction}, id {direction}", key.sql()),
        }
    }
}

/// `?, ?, ?` for `count` values, for an SQL `IN` list.
fn marks(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Criteria for the games table. Unset fields do not restrict.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub player: String,
    /// Other names of the same player, such as their Lichess and Chess.com accounts,
    /// matched together with `player`.
    pub player_aliases: Vec<String>,
    pub side: SideFilter,
    pub eco_min: String,
    pub eco_max: String,
    pub min_opponent_rating: Option<i64>,
    pub time_classes: Vec<TimeClass>,
    /// Only games reaching this position.
    pub position: Option<u64>,
    /// Exact pawn arrangement and material counts, independent of side to move.
    pub pawn_structure: Option<u64>,
    pub material: Option<u64>,
    pub only_left_repertoire: bool,
    /// Only games with comments, NAGs or variations.
    pub only_annotated: bool,
    /// Earliest and latest date, in the stored form from [`date_bound`].
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// Text the event or site contains.
    pub event: String,
    pub result: Option<ResultFilter>,
    /// The player's rating range; without a player, both players' ratings.
    pub min_rating: Option<i64>,
    pub max_rating: Option<i64>,
    /// Game length in moves.
    pub min_moves: Option<i64>,
    pub max_moves: Option<i64>,
    /// The order of the results.
    pub sort: Sort,
}

impl Filter {
    /// The player's names: `player` and its aliases, trimmed, without empty ones.
    fn player_names(&self) -> Vec<String> {
        let player = self.player.trim();
        if player.is_empty() {
            return Vec::new();
        }
        let mut names = vec![player.to_owned()];
        for alias in &self.player_aliases {
            let alias = alias.trim();
            if !alias.is_empty() && !names.iter().any(|name| name.eq_ignore_ascii_case(alias)) {
                names.push(alias.to_owned());
            }
        }
        names
    }

    fn sql(&self) -> (String, Vec<Box<dyn ToSql>>) {
        let mut clauses: Vec<String> = Vec::new();
        let mut values: Vec<Box<dyn ToSql>> = Vec::new();
        let mut pattern_clauses = Vec::new();
        for (column, value) in [
            ("pawn_hash", self.pawn_structure),
            ("material", self.material),
        ] {
            if let Some(value) = value {
                pattern_clauses.push(format!("{column} = ?"));
                values.push(Box::new(value as i64));
            }
        }
        if !pattern_clauses.is_empty() {
            clauses.push(format!(
                "id IN (SELECT game_id FROM structure_positions WHERE {})",
                pattern_clauses.join(" AND ")
            ));
        }
        let names = self.player_names();
        let push_names = |values: &mut Vec<Box<dyn ToSql>>| {
            for name in &names {
                values.push(Box::new(name.clone()));
            }
        };
        if !names.is_empty() {
            let list = marks(names.len());
            let white = format!("white COLLATE NOCASE IN ({list})");
            let black = format!("black COLLATE NOCASE IN ({list})");
            match self.side {
                SideFilter::Any => {
                    clauses.push(format!("({white} OR {black})"));
                    push_names(&mut values);
                    push_names(&mut values);
                }
                SideFilter::White => {
                    clauses.push(white);
                    push_names(&mut values);
                }
                SideFilter::Black => {
                    clauses.push(black);
                    push_names(&mut values);
                }
            }
            if let Some(rating) = self.min_opponent_rating {
                clauses.push(format!(
                    "CASE WHEN white COLLATE NOCASE IN ({list}) THEN COALESCE(black_elo, 0)
                          ELSE COALESCE(white_elo, 0) END >= ?"
                ));
                push_names(&mut values);
                values.push(Box::new(rating));
            }
        } else if let Some(rating) = self.min_opponent_rating {
            clauses.push("MIN(COALESCE(white_elo, 0), COALESCE(black_elo, 0)) >= ?".to_owned());
            values.push(Box::new(rating));
        }
        if !self.eco_min.is_empty() {
            clauses.push("eco >= ?".to_owned());
            values.push(Box::new(self.eco_min.clone()));
        }
        if !self.eco_max.is_empty() {
            clauses.push("eco <= ?".to_owned());
            values.push(Box::new(self.eco_max.clone()));
        }
        if !self.time_classes.is_empty() {
            let marks = vec!["?"; self.time_classes.len()].join(", ");
            clauses.push(format!("time_class IN ({marks})"));
            for class in &self.time_classes {
                values.push(Box::new(class.as_str()));
            }
        }
        if let Some(hash) = self.position {
            clauses.push("id IN (SELECT game_id FROM positions WHERE hash = ?)".to_owned());
            values.push(Box::new(hash as i64));
        }
        if self.only_left_repertoire {
            clauses.push("left_rep IS NOT NULL".to_owned());
        }
        if self.only_annotated {
            clauses.push("annotated IS NOT NULL".to_owned());
        }
        if let Some(from) = &self.date_from {
            clauses.push("date >= ?".to_owned());
            values.push(Box::new(from.clone()));
        }
        if let Some(to) = &self.date_to {
            clauses.push("date <= ?".to_owned());
            values.push(Box::new(to.clone()));
        }
        let event = self.event.trim();
        if !event.is_empty() {
            clauses.push("(event LIKE ? ESCAPE '\\' OR site LIKE ? ESCAPE '\\')".to_owned());
            values.push(Box::new(contains_pattern(event)));
            values.push(Box::new(contains_pattern(event)));
        }
        let list = marks(names.len());
        if let Some(result) = self.result {
            if names.is_empty() {
                let stored = match result {
                    ResultFilter::Win => "1-0",
                    ResultFilter::Draw => "1/2-1/2",
                    ResultFilter::Loss => "0-1",
                };
                clauses.push("result = ?".to_owned());
                values.push(Box::new(stored));
            } else if result == ResultFilter::Draw {
                clauses.push("result = '1/2-1/2'".to_owned());
            } else {
                let (as_white, as_black) = if result == ResultFilter::Win {
                    ("1-0", "0-1")
                } else {
                    ("0-1", "1-0")
                };
                clauses.push(format!(
                    "((white COLLATE NOCASE IN ({list}) AND result = '{as_white}')
                      OR (black COLLATE NOCASE IN ({list}) AND result = '{as_black}'))"
                ));
                push_names(&mut values);
                push_names(&mut values);
            }
        }
        if self.min_rating.is_some() || self.max_rating.is_some() {
            let (low, high) = if names.is_empty() {
                (
                    "MIN(COALESCE(white_elo, 0), COALESCE(black_elo, 0))".to_owned(),
                    "MAX(COALESCE(white_elo, 0), COALESCE(black_elo, 0))".to_owned(),
                )
            } else {
                let own = format!(
                    "(CASE WHEN white COLLATE NOCASE IN ({list}) THEN COALESCE(white_elo, 0)
                           ELSE COALESCE(black_elo, 0) END)"
                );
                (own.clone(), own)
            };
            if let Some(min) = self.min_rating {
                clauses.push(format!("{low} >= ?"));
                if !names.is_empty() {
                    push_names(&mut values);
                }
                values.push(Box::new(min));
            }
            if let Some(max) = self.max_rating {
                clauses.push(format!("{high} <= ?"));
                if !names.is_empty() {
                    push_names(&mut values);
                }
                values.push(Box::new(max));
            }
        }
        if let Some(min) = self.min_moves {
            clauses.push("(plies + 1) / 2 >= ?".to_owned());
            values.push(Box::new(min));
        }
        if let Some(max) = self.max_moves {
            clauses.push("(plies + 1) / 2 <= ?".to_owned());
            values.push(Box::new(max));
        }
        let clause = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        (clause, values)
    }
}

/// A game for repertoire analysis: id, moves, whether the player had White, their score.
pub type PlayerLine = (i64, Vec<String>, bool, f32);

/// One row of the games table.
#[derive(Clone, Debug, PartialEq)]
pub struct GameRow {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub white_elo: Option<i64>,
    pub black_elo: Option<i64>,
    pub result: String,
    pub date: String,
    pub eco: String,
    pub event: String,
    pub round: String,
    pub source: String,
    pub plies: i64,
    pub accuracy: Option<f64>,
    pub left_rep: Option<String>,
    /// The game has comments, NAGs or variations.
    pub annotated: bool,
}

impl GameRow {
    pub fn moves_count(&self) -> i64 {
        (self.plies + 1) / 2
    }

    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            white: row.get(1)?,
            black: row.get(2)?,
            white_elo: row.get(3)?,
            black_elo: row.get(4)?,
            result: row.get(5)?,
            date: row.get(6)?,
            eco: row.get(7)?,
            event: row.get(8)?,
            round: row.get(9)?,
            source: row.get(10)?,
            plies: row.get(11)?,
            accuracy: row.get(12)?,
            left_rep: row.get(13)?,
            annotated: row.get(14)?,
        })
    }
}

const ROW_COLUMNS: &str = "id, white, black, white_elo, black_elo, result, date, eco, event, round, source, plies, accuracy, left_rep, annotated IS NOT NULL";

/// Row data plus the move list, for preview and opening a game.
#[derive(Clone, Debug, PartialEq)]
pub struct GameDetails {
    pub row: GameRow,
    /// Empty means the standard initial position for databases created before custom
    /// starting positions were retained.
    pub start_fen: String,
    /// Whether engine searches and PGN export use Chess960 castling rules.
    pub chess960: bool,
    pub sans: Vec<String>,
    /// The movetext with comments, variations and NAGs, without the result. Set when
    /// the game was imported with them or saved from Analyse; plain games keep `None`.
    pub annotated: Option<String>,
    pub review_summary: Option<ReviewSummary>,
    pub review_positions: Vec<ReviewPosition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewSummary {
    pub job_key: String,
    pub engine: String,
    pub search_limit: String,
    pub white_accuracy: Option<f64>,
    pub black_accuracy: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewPosition {
    pub ply: u32,
    pub fen: String,
    pub eval_cp: i32,
    pub mate_distance: Option<i32>,
    pub depth: u32,
    pub pv: String,
}

impl GameDetails {
    /// A minimal PGN of the stored game.
    pub fn to_pgn(&self) -> String {
        let row = &self.row;
        let mut out = String::new();
        let mut tag = |name: &str, value: &str| {
            out.push_str(&format!("[{name} \"{}\"]\n", value.replace('"', "'")));
        };
        tag(
            "Event",
            if row.event.is_empty() {
                "?"
            } else {
                &row.event
            },
        );
        tag("Date", &row.date.replace('-', "."));
        tag(
            "Round",
            if row.round.is_empty() {
                "?"
            } else {
                &row.round
            },
        );
        tag("White", &row.white);
        tag("Black", &row.black);
        tag("Result", &row.result);
        if self.chess960 {
            tag("Variant", "Chess960");
        }
        if !self.start_fen.is_empty() {
            tag("SetUp", "1");
            tag("FEN", &self.start_fen);
        }
        if let Some(elo) = row.white_elo {
            tag("WhiteElo", &elo.to_string());
        }
        if let Some(elo) = row.black_elo {
            tag("BlackElo", &elo.to_string());
        }
        if !row.eco.is_empty() {
            tag("ECO", &row.eco);
        }
        out.push('\n');
        if let Some(movetext) = &self.annotated {
            out.push_str(movetext);
            out.push(' ');
            out.push_str(&row.result);
            out.push('\n');
            return out;
        }
        for (ply, san) in self.sans.iter().enumerate() {
            if ply % 2 == 0 {
                out.push_str(&format!("{}. ", ply / 2 + 1));
            }
            out.push_str(san);
            out.push(' ');
        }
        out.push_str(&row.result);
        out.push('\n');
        out
    }
}

/// Aggregate of a move played from a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveStat {
    pub san: String,
    pub games: u64,
    pub white_wins: u64,
    pub draws: u64,
    pub black_wins: u64,
}

impl MoveStat {
    /// Score percentage for the given side.
    pub fn score_percent(&self, for_white: bool) -> f32 {
        let wins = if for_white {
            self.white_wins
        } else {
            self.black_wins
        };
        (wins as f32 + self.draws as f32 / 2.0) * 100.0 / self.games.max(1) as f32
    }
}

/// Headline numbers for a filtered set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterStats {
    pub games: u64,
    /// Score for the filtered player, or for White if no player is set.
    pub score_percent: f32,
    /// Most frequent deviation (`left_rep` value) and its count.
    pub top_leak: Option<(String, u64)>,
}

impl Database {
    /// Ids matching the filter, in the filter's sort order. Cheap enough for 1M+ rows
    /// and lets the table virtualize by fetching row data only for the visible range.
    pub fn filtered_ids(&self, filter: &Filter) -> Result<Vec<i64>> {
        let (clause, values) = filter.sql();
        let order = filter.sort.order_by(!clause.is_empty());
        let mut statement = self
            .conn
            .prepare(&format!("SELECT id FROM games{clause} ORDER BY {order}"))?;
        let rows = statement.query_map(params_from_iter(values.iter()), |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Player names containing `query`, for suggestions while typing a player filter.
    /// Names starting with it come first, then the names with the most games. Each name
    /// comes with its game count.
    pub fn player_names(&self, query: &str, limit: usize) -> Result<Vec<(String, u64)>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let mut statement = self.conn.prepare(
            "SELECT name, COUNT(*) AS games FROM (
                SELECT white AS name FROM games WHERE white LIKE ?1 ESCAPE '\\'
                UNION ALL
                SELECT black FROM games WHERE black LIKE ?1 ESCAPE '\\'
             )
             GROUP BY name COLLATE NOCASE
             ORDER BY name LIKE ?2 ESCAPE '\\' DESC, games DESC, name
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![format!("%{escaped}%"), format!("{escaped}%"), limit as i64],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)),
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Finds a small set of games by player, event or opening code for global search.
    pub fn search_games(&self, query: &str, limit: usize) -> Result<Vec<GameRow>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let pattern = format!(
            "%{}%",
            query
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut statement = self.conn.prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM games
             WHERE white LIKE ?1 ESCAPE '\\' OR black LIKE ?1 ESCAPE '\\'
                OR event LIKE ?1 ESCAPE '\\' OR eco LIKE ?1 ESCAPE '\\'
             ORDER BY date DESC, id DESC LIMIT ?2"
        ))?;
        Ok(statement
            .query_map(params![pattern, limit as i64], GameRow::from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn rows_by_ids(&self, ids: &[i64]) -> Result<Vec<GameRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        let mut statement = self.conn.prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM games WHERE id IN ({marks})"
        ))?;
        let mut found: Vec<GameRow> = statement
            .query_map(params_from_iter(ids.iter()), GameRow::from_row)?
            .collect::<rusqlite::Result<_>>()?;
        // Preserve the caller's order.
        found.sort_by_key(|row| ids.iter().position(|&id| id == row.id));
        Ok(found)
    }

    pub fn game(&self, id: i64) -> Result<Option<GameDetails>> {
        let mut found = self
            .conn
            .query_row(
                &format!(
                    "SELECT {ROW_COLUMNS}, moves, start_fen, annotated, chess960 FROM games WHERE id = ?1"
                ),
                params![id],
                |row| {
                    let moves: String = row.get(15)?;
                    let start_fen: String = row.get(16)?;
                    let annotated: Option<String> = row.get(17)?;
                    Ok(GameDetails {
                        row: GameRow::from_row(row)?,
                        start_fen,
                        chess960: row.get(18)?,
                        annotated,
                        sans: moves.split_ascii_whitespace().map(str::to_owned).collect(),
                        review_summary: None,
                        review_positions: Vec::new(),
                    })
                },
            )
            .optional()?;
        if let Some(game) = &mut found {
            game.review_summary = self.review_summary(id)?;
            if let Some(summary) = &game.review_summary {
                game.review_positions = self.review_positions(id, &summary.job_key)?;
            }
        }
        Ok(found)
    }

    pub fn review_positions(&self, game_id: i64, job_key: &str) -> Result<Vec<ReviewPosition>> {
        let mut statement = self.conn.prepare(
            "SELECT ply,fen,eval_cp,mate_distance,depth,pv FROM review_positions
             WHERE game_id=?1 AND job_key=?2 ORDER BY ply",
        )?;
        let rows = statement.query_map(params![game_id, job_key], |row| {
            Ok(ReviewPosition {
                ply: row.get::<_, i64>(0)? as u32,
                fen: row.get(1)?,
                eval_cp: row.get(2)?,
                mate_distance: row.get(3)?,
                depth: row.get::<_, i64>(4)? as u32,
                pv: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_review_position(
        &self,
        game_id: i64,
        job_key: &str,
        result: &ReviewPosition,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO review_positions(game_id,job_key,ply,fen,eval_cp,mate_distance,depth,pv)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(game_id,job_key,ply) DO UPDATE SET
             fen=excluded.fen,eval_cp=excluded.eval_cp,mate_distance=excluded.mate_distance,
             depth=excluded.depth,pv=excluded.pv",
            params![
                game_id,
                job_key,
                result.ply as i64,
                result.fen,
                result.eval_cp,
                result.mate_distance,
                result.depth as i64,
                result.pv
            ],
        )?;
        Ok(())
    }

    pub fn set_review_summary(
        &self,
        game_id: i64,
        job_key: &str,
        summary: &ReviewSummary,
        updated_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO review_summaries(game_id,job_key,engine,search_limit,white_accuracy,black_accuracy,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(game_id,job_key) DO UPDATE SET
             engine=excluded.engine,search_limit=excluded.search_limit,white_accuracy=excluded.white_accuracy,
             black_accuracy=excluded.black_accuracy,updated_at=excluded.updated_at",
            params![game_id, job_key, summary.engine, summary.search_limit, summary.white_accuracy, summary.black_accuracy, updated_at],
        )?;
        Ok(())
    }

    fn review_summary(&self, game_id: i64) -> Result<Option<ReviewSummary>> {
        Ok(self
            .conn
            .query_row(
                "SELECT job_key,engine,search_limit,white_accuracy,black_accuracy FROM review_summaries
             WHERE game_id=?1 ORDER BY updated_at DESC LIMIT 1",
                [game_id],
                |row| {
                    Ok(ReviewSummary {
                        job_key: row.get(0)?,
                        engine: row.get(1)?,
                        search_limit: row.get(2)?,
                        white_accuracy: row.get(3)?,
                        black_accuracy: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn stats(&self, filter: &Filter) -> Result<FilterStats> {
        let (clause, values) = filter.sql();
        let names = filter.player_names();
        // Score from the filtered player's perspective, else White's.
        let (expression, player_values) = if names.is_empty() {
            (
                "SUM(CASE result WHEN '1-0' THEN 1.0 WHEN '1/2-1/2' THEN 0.5 ELSE 0.0 END)"
                    .to_owned(),
                vec![],
            )
        } else {
            let list = marks(names.len());
            (
                format!(
                    "SUM(CASE WHEN result = '1/2-1/2' THEN 0.5
                          WHEN result = '1-0' AND white COLLATE NOCASE IN ({list}) THEN 1.0
                          WHEN result = '0-1' AND black COLLATE NOCASE IN ({list}) THEN 1.0
                          ELSE 0.0 END)"
                ),
                names.iter().chain(names.iter()).cloned().collect(),
            )
        };
        let mut all: Vec<&dyn ToSql> = player_values.iter().map(|v| v as &dyn ToSql).collect();
        all.extend(values.iter().map(AsRef::as_ref));
        let (games, score): (i64, Option<f64>) = self.conn.query_row(
            &format!("SELECT COUNT(*), {expression} FROM games{clause}"),
            params_from_iter(all.iter()),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let joiner = if clause.is_empty() { " WHERE" } else { " AND" };
        let games = games as u64;
        let top_leak = self
            .conn
            .query_row(
                &format!(
                    "SELECT left_rep, COUNT(*) AS n FROM games{clause}{joiner} left_rep IS NOT NULL
                     GROUP BY left_rep ORDER BY n DESC, left_rep LIMIT 1"
                ),
                params_from_iter(values.iter()),
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)),
            )
            .optional()?;
        Ok(FilterStats {
            games,
            score_percent: (score.unwrap_or(0.0) * 100.0 / games.max(1) as f64) as f32,
            top_leak,
        })
    }

    /// Moves played from a position, most popular first.
    pub fn moves_from(&self, hash: u64) -> Result<Vec<MoveStat>> {
        self.moves_from_filtered(hash, &[])
    }

    /// Like [`Self::moves_from`] but only counting games where one of `players` (names
    /// of the same person) took part.
    pub fn moves_from_players(&self, hash: u64, players: &[String]) -> Result<Vec<MoveStat>> {
        self.moves_from_filtered(hash, players)
    }

    fn moves_from_filtered(&self, hash: u64, players: &[String]) -> Result<Vec<MoveStat>> {
        let list = (2..players.len() + 2)
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let player_clause = if players.is_empty() {
            String::new()
        } else {
            format!(
                "AND (g.white COLLATE NOCASE IN ({list}) OR g.black COLLATE NOCASE IN ({list}))"
            )
        };
        let sql = format!(
            "SELECT p.next_san, COUNT(*) AS n,
                    SUM(g.result = '1-0'), SUM(g.result = '1/2-1/2'), SUM(g.result = '0-1')
             FROM positions p JOIN games g ON g.id = p.game_id
             WHERE p.hash = ?1 AND p.next_san IS NOT NULL {player_clause}
             GROUP BY p.next_san ORDER BY n DESC, p.next_san"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let map = |row: &Row<'_>| {
            Ok(MoveStat {
                san: row.get(0)?,
                games: row.get::<_, i64>(1)? as u64,
                white_wins: row.get::<_, i64>(2)? as u64,
                draws: row.get::<_, i64>(3)? as u64,
                black_wins: row.get::<_, i64>(4)? as u64,
            })
        };
        let mut values: Vec<&dyn ToSql> = Vec::new();
        let hash = hash as i64;
        values.push(&hash);
        for player in players {
            values.push(player);
        }
        let rows: rusqlite::Result<Vec<MoveStat>> = statement
            .query_map(params_from_iter(values.iter()), map)?
            .collect();
        Ok(rows?)
    }

    pub fn set_left_rep(&self, id: i64, left_rep: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET left_rep = ?2 WHERE id = ?1",
            params![id, left_rep],
        )?;
        Ok(())
    }

    /// The moves of any game with this deviation, to show the leak in context.
    pub fn sample_with_left_rep(&self, left_rep: &str) -> Result<Option<Vec<String>>> {
        let moves: Option<String> = self
            .conn
            .query_row(
                "SELECT moves FROM games WHERE left_rep = ?1 LIMIT 1",
                params![left_rep],
                |row| row.get(0),
            )
            .optional()?;
        Ok(moves.map(|moves| moves.split_ascii_whitespace().map(str::to_owned).collect()))
    }

    pub fn set_accuracy(&self, id: i64, accuracy: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET accuracy = ?2 WHERE id = ?1",
            params![id, accuracy],
        )?;
        Ok(())
    }

    /// Every game's id, moves and colour relative to the player, for repertoire
    /// analysis. `players` are names of the same person.
    pub fn player_lines(&self, players: &[String]) -> Result<Vec<PlayerLine>> {
        if players.is_empty() {
            return Ok(Vec::new());
        }
        let list = marks(players.len());
        let mut statement = self.conn.prepare(&format!(
            "SELECT id, moves, white COLLATE NOCASE IN ({list}), result FROM games
             WHERE white COLLATE NOCASE IN ({list}) OR black COLLATE NOCASE IN ({list})"
        ))?;
        let values: Vec<&String> = players.iter().chain(players).chain(players).collect();
        let rows = statement.query_map(params_from_iter(values), |row| {
            let id: i64 = row.get(0)?;
            let moves: String = row.get(1)?;
            let white: bool = row.get(2)?;
            let result: String = row.get(3)?;
            let score = match (result.as_str(), white) {
                ("1-0", true) | ("0-1", false) => 1.0,
                ("1/2-1/2", _) => 0.5,
                _ => 0.0,
            };
            let sans = moves.split_ascii_whitespace().map(str::to_owned).collect();
            Ok((id, sans, white, score))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Runs `body` in a single transaction; fast path for bulk updates.
    pub fn in_transaction<T>(&mut self, body: impl FnOnce(&Database) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("BEGIN")?;
        match body(self) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImportControl, ImportOptions, import_games};
    use chess_core::{Chess, Position, position_hash, shakmaty::san::San};

    const GAMES: &str = r#"[White "me"]
[Black "alice"]
[WhiteElo "2000"]
[BlackElo "1950"]
[Result "1-0"]
[Date "2026.01.02"]
[ECO "B90"]
[TimeControl "600+5"]

1. e4 c5 2. Nf3 d6 1-0

[White "bob"]
[Black "me"]
[WhiteElo "1800"]
[BlackElo "2000"]
[Result "1/2-1/2"]
[Date "2026.01.03"]
[ECO "C50"]
[TimeControl "180+0"]

1. e4 e5 2. Nf3 Nc6 1/2-1/2

[White "me"]
[Black "carol"]
[WhiteElo "2010"]
[BlackElo "2200"]
[Result "0-1"]
[Date "2026.01.01"]
[ECO "B99"]
[TimeControl "5400+30"]
[Site "https://lichess.org/abc"]

1. e4 c5 2. c3 d5 0-1
"#;

    fn database() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let summary = import_games(
            &path,
            GAMES.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(summary.games, 3);
        (dir, Database::open(&path).unwrap())
    }

    fn hash_after(moves: &[&str]) -> u64 {
        let mut pos = Chess::default();
        for san in moves {
            let mv = san.parse::<San>().unwrap().to_move(&pos).unwrap();
            pos.play_unchecked(mv);
        }
        position_hash(&pos)
    }

    /// Global search accepts partial names and opening codes without treating wildcards as input syntax.
    #[test]
    fn global_search_matches_partial_names_and_codes() {
        let (_dir, db) = database();
        assert_eq!(db.search_games("ALI", 8).unwrap()[0].black, "alice");
        assert_eq!(db.search_games("B9", 8).unwrap().len(), 2);
        assert_eq!(db.search_games("me", 1).unwrap().len(), 1);
        assert!(db.search_games("%", 8).unwrap().is_empty());
        assert!(db.search_games("_", 8).unwrap().is_empty());
        assert!(db.search_games("  ", 8).unwrap().is_empty());
        assert!(db.search_games("me", 0).unwrap().is_empty());
    }

    #[test]
    fn newest_first_and_pages_keep_order() {
        let (_dir, db) = database();
        let ids = db.filtered_ids(&Filter::default()).unwrap();
        assert_eq!(ids.len(), 3);
        let rows = db.rows_by_ids(&ids).unwrap();
        assert_eq!(rows[0].white, "bob");
        assert_eq!(rows[1].black, "alice");
        assert_eq!(rows[2].source, "Online");
        assert_eq!(rows[1].source, "OTB");
        assert_eq!(rows[1].moves_count(), 2);
    }

    #[test]
    fn filters_combine() {
        let (_dir, db) = database();
        let count = |filter: Filter| db.filtered_ids(&filter).unwrap().len();
        let me = Filter {
            player: "ME".to_owned(),
            ..Default::default()
        };
        assert_eq!(count(me.clone()), 3);
        assert_eq!(
            count(Filter {
                side: SideFilter::White,
                ..me.clone()
            }),
            2
        );
        assert_eq!(
            count(Filter {
                side: SideFilter::Black,
                ..me.clone()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                min_opponent_rating: Some(1900),
                ..me.clone()
            }),
            2
        );
        let aliases = Filter {
            player: "alice".to_owned(),
            player_aliases: vec!["Bob".to_owned(), " ".to_owned()],
            ..Default::default()
        };
        assert_eq!(count(aliases.clone()), 2);
        assert_eq!(
            count(Filter {
                side: SideFilter::White,
                ..aliases.clone()
            }),
            1
        );
        let stats = db.stats(&aliases).unwrap();
        assert!((stats.score_percent - 25.0).abs() < 0.01);
        assert_eq!(
            count(Filter {
                eco_min: "B90".to_owned(),
                eco_max: "B99".to_owned(),
                ..Default::default()
            }),
            2
        );
        assert_eq!(
            count(Filter {
                time_classes: vec![TimeClass::Rapid, TimeClass::Classical],
                ..Default::default()
            }),
            2
        );
        assert_eq!(
            count(Filter {
                position: Some(hash_after(&["e4", "c5"])),
                ..Default::default()
            }),
            2
        );
    }

    #[test]
    fn position_search_finds_transpositions_and_next_moves() {
        let (_dir, db) = database();
        let moves = db.moves_from(hash_after(&["e4", "c5"])).unwrap();
        assert_eq!(moves.len(), 2);
        let nf3 = moves.iter().find(|stat| stat.san == "Nf3").unwrap();
        assert_eq!(
            (nf3.games, nf3.white_wins, nf3.draws, nf3.black_wins),
            (1, 1, 0, 0)
        );
        assert!((nf3.score_percent(true) - 100.0).abs() < f32::EPSILON);
        let start = db.moves_from(hash_after(&[])).unwrap();
        assert_eq!(start[0].san, "e4");
        assert_eq!(start[0].games, 3);
        assert!((start[0].score_percent(true) - 50.0).abs() < f32::EPSILON);
        let bob = db
            .moves_from_players(hash_after(&[]), &["bob".to_owned()])
            .unwrap();
        assert_eq!(bob[0].games, 1);
        let both = db
            .moves_from_players(hash_after(&[]), &["bob".to_owned(), "CAROL".to_owned()])
            .unwrap();
        assert_eq!(both[0].games, 2);
    }

    #[test]
    fn stats_are_from_the_players_perspective() {
        let (_dir, db) = database();
        let stats = db
            .stats(&Filter {
                player: "me".to_owned(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(stats.games, 3);
        assert!((stats.score_percent - 50.0).abs() < 0.01);
        assert_eq!(stats.top_leak, None);
        let ids = db.filtered_ids(&Filter::default()).unwrap();
        db.set_left_rep(ids[0], Some("14.h4")).unwrap();
        db.set_left_rep(ids[1], Some("14.h4")).unwrap();
        let stats = db.stats(&Filter::default()).unwrap();
        assert_eq!(stats.top_leak, Some(("14.h4".to_owned(), 2)));
        let only = Filter {
            only_left_repertoire: true,
            ..Default::default()
        };
        assert_eq!(db.filtered_ids(&only).unwrap().len(), 2);
    }

    #[test]
    fn game_details_round_trip_to_pgn() {
        let (_dir, db) = database();
        let ids = db.filtered_ids(&Filter::default()).unwrap();
        let details = db.game(ids[0]).unwrap().unwrap();
        assert_eq!(details.sans, vec!["e4", "e5", "Nf3", "Nc6"]);
        let tree = chess_core::parse_pgn(&details.to_pgn()).unwrap().remove(0);
        assert_eq!(tree.header("White"), Some("bob"));
        assert_eq!(tree.mainline().len(), 4);
        assert!(db.game(9_999).unwrap().is_none());
    }

    #[test]
    fn player_names_suggest_prefixes_first_with_counts() {
        let (_dir, db) = database();
        let names = db.player_names("a", 10).unwrap();
        assert_eq!(names[0], ("alice".to_owned(), 1));
        assert!(names.contains(&("carol".to_owned(), 1)));
        assert!(!names.iter().any(|(name, _)| name == "bob"));
        assert_eq!(
            db.player_names("ME", 10).unwrap(),
            vec![("me".to_owned(), 3)]
        );
        assert!(db.player_names("%", 10).unwrap().is_empty());
        assert!(db.player_names("  ", 10).unwrap().is_empty());
    }

    #[test]
    fn player_lines_report_colour_and_score() {
        let (_dir, db) = database();
        let mut lines = db.player_lines(&["me".to_owned()]).unwrap();
        lines.sort_by_key(|(id, ..)| *id);
        assert!(lines[0].2);
        assert_eq!(lines[0].3, 1.0);
        assert!(!lines[1].2);
        assert_eq!(lines[1].3, 0.5);
    }

    #[test]
    fn date_bounds_pad_partial_dates() {
        assert_eq!(date_bound("2020", false).as_deref(), Some("2020-00-00"));
        assert_eq!(date_bound("2020", true).as_deref(), Some("2020-~~-~~"));
        assert_eq!(date_bound("2020.05", true).as_deref(), Some("2020-05-~~"));
        assert_eq!(
            date_bound("2020-05-17", false).as_deref(),
            Some("2020-05-17")
        );
        for bad in [
            "",
            "20",
            "2020.5",
            "2020.13",
            "2020.05.32",
            "abc",
            "2020.01.01.01",
        ] {
            assert_eq!(date_bound(bad, false), None, "{bad}");
        }
    }

    #[test]
    fn metadata_filters_alone_and_combined() {
        let (_dir, db) = database();
        let count = |filter: Filter| db.filtered_ids(&filter).unwrap().len();
        let dates = |from: &str, to: &str| Filter {
            date_from: date_bound(from, false),
            date_to: date_bound(to, true),
            ..Default::default()
        };
        assert_eq!(count(dates("2026.01.02", "2026")), 2);
        assert_eq!(count(dates("2025", "2026.01.01")), 1);
        assert_eq!(count(dates("2027", "2027")), 0);
        assert_eq!(
            count(Filter {
                event: "LICHESS".to_owned(),
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                event: "%".to_owned(),
                ..Default::default()
            }),
            0
        );
        let me = |result| Filter {
            player: "me".to_owned(),
            result: Some(result),
            ..Default::default()
        };
        // "me" won with White, drew with Black and lost with White.
        assert_eq!(db.filtered_ids(&me(ResultFilter::Win)).unwrap(), vec![1]);
        assert_eq!(db.filtered_ids(&me(ResultFilter::Draw)).unwrap(), vec![2]);
        assert_eq!(db.filtered_ids(&me(ResultFilter::Loss)).unwrap(), vec![3]);
        let bob_loss = Filter {
            player: "bob".to_owned(),
            result: Some(ResultFilter::Loss),
            ..Default::default()
        };
        assert_eq!(count(bob_loss), 0);
        let white_wins = Filter {
            result: Some(ResultFilter::Win),
            ..Default::default()
        };
        assert_eq!(db.filtered_ids(&white_wins).unwrap(), vec![1]);
        assert_eq!(
            count(Filter {
                player: "me".to_owned(),
                min_rating: Some(2005),
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                min_rating: Some(1900),
                max_rating: Some(2100),
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                min_moves: Some(2),
                max_moves: Some(2),
                ..Default::default()
            }),
            3
        );
        assert_eq!(
            count(Filter {
                max_moves: Some(1),
                ..Default::default()
            }),
            0
        );
        let combined = Filter {
            player: "me".to_owned(),
            side: SideFilter::White,
            result: Some(ResultFilter::Loss),
            date_from: date_bound("2026", false),
            event: "lichess".to_owned(),
            ..Default::default()
        };
        assert_eq!(db.filtered_ids(&combined).unwrap(), vec![3]);
        assert_eq!(db.stats(&combined).unwrap().games, 1);
    }

    #[test]
    fn unknown_date_parts_fall_inside_a_year() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("q.sqlite");
        let pgn = "[Date \"2019.??.??\"]\n\n1. e4 *\n\n[Date \"????.??.??\"]\n\n1. d4 *\n";
        import_games(
            &path,
            pgn.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        let db = Database::open(&path).unwrap();
        let year = Filter {
            date_from: date_bound("2019", false),
            date_to: date_bound("2019", true),
            ..Default::default()
        };
        assert_eq!(db.filtered_ids(&year).unwrap(), vec![1]);
    }

    #[test]
    fn every_sort_key_orders_both_ways_with_ties_by_id() {
        let (_dir, db) = database();
        let ids = |key: SortKey, descending: bool| {
            db.filtered_ids(&Filter {
                sort: Sort { key, descending },
                ..Default::default()
            })
            .unwrap()
        };
        let mut file = ids(SortKey::FileOrder, false);
        assert_eq!(file, vec![1, 2, 3]);
        file.reverse();
        assert_eq!(ids(SortKey::FileOrder, true), file);
        let white = |ids: Vec<i64>| -> Vec<String> {
            db.rows_by_ids(&ids)
                .unwrap()
                .into_iter()
                .map(|row| row.white)
                .collect()
        };
        assert_eq!(white(ids(SortKey::White, false)), ["bob", "me", "me"]);
        // "me" ties: file order, reversed when descending.
        assert_eq!(ids(SortKey::White, false), vec![2, 1, 3]);
        assert_eq!(ids(SortKey::White, true), vec![3, 1, 2]);
        assert_eq!(ids(SortKey::Black, false), vec![1, 3, 2]);
        assert_eq!(ids(SortKey::Result, false), vec![3, 1, 2]);
        assert_eq!(ids(SortKey::Date, false), vec![3, 1, 2]);
        assert_eq!(ids(SortKey::Date, true), vec![2, 1, 3]);
        assert_eq!(ids(SortKey::Source, true), vec![2, 1, 3]);
        assert_eq!(ids(SortKey::Moves, false), vec![1, 2, 3]);
        assert_eq!(ids(SortKey::Moves, true), vec![3, 2, 1]);
        db.set_accuracy(2, 91.0).unwrap();
        db.set_accuracy(3, 75.0).unwrap();
        // Games without accuracy come first ascending and last descending.
        assert_eq!(ids(SortKey::Accuracy, false), vec![1, 3, 2]);
        assert_eq!(ids(SortKey::Accuracy, true), vec![2, 3, 1]);
        for key in SortKey::ALL {
            assert_eq!(SortKey::from_name(key.as_str()), Some(key));
            assert_eq!(ids(key, false).len(), 3);
            assert_eq!(ids(key, true).len(), 3);
        }
    }

    #[test]
    fn time_classes() {
        assert_eq!(TimeClass::from_time_control("60+0"), TimeClass::Bullet);
        assert_eq!(TimeClass::from_time_control("180+2"), TimeClass::Blitz);
        assert_eq!(TimeClass::from_time_control("600+5"), TimeClass::Rapid);
        assert_eq!(
            TimeClass::from_time_control("5400+30"),
            TimeClass::Classical
        );
        assert_eq!(TimeClass::from_time_control("-"), TimeClass::Unknown);
    }
}
