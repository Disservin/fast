//! The New tournament form as fastchess arguments.

use super::*;

/// One engine in a tournament.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Participant {
    pub name: String,
    pub cmd: PathBuf,
    pub options: BTreeMap<String, String>,
}

/// Options Fast saves for analysis that would distort a match, or are set for every engine.
pub(super) const SKIPPED_OPTIONS: [&str; 5] =
    ["Hash", "Threads", "MultiPV", "UCI_AnalyseMode", "Ponder"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    TimeControl,
    MoveTime,
    Nodes,
    Depth,
}

impl Limit {
    pub const ALL: [Self; 4] = [Self::TimeControl, Self::MoveTime, Self::Nodes, Self::Depth];

    pub fn label(self) -> &'static str {
        match self {
            Self::TimeControl => "Clock",
            Self::MoveTime => "Per move",
            Self::Nodes => "Nodes",
            Self::Depth => "Depth",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::TimeControl => "Seconds+increment, e.g. 10+0.1 or 40/60",
            Self::MoveTime => "Seconds per move, e.g. 0.5",
            Self::Nodes => "Nodes per move, e.g. 100000",
            Self::Depth => "Plies per move, e.g. 12",
        }
    }
}

/// How fastchess writes the games file: the move notation and what each move's comment records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PgnOut {
    /// `san`, `lan` or `uci`.
    pub notation: &'static str,
    pub nodes: bool,
    pub seldepth: bool,
    pub nps: bool,
    pub hashfull: bool,
    pub tbhits: bool,
    pub pv: bool,
    pub timeleft: bool,
    pub latency: bool,
    /// Minimal PGN: fewer headers and no move comments.
    pub min: bool,
    /// Engine output lines matching this regex are copied into the PGN.
    pub match_line: String,
}

impl Default for PgnOut {
    fn default() -> Self {
        Self {
            notation: "san",
            nodes: false,
            seldepth: false,
            nps: false,
            hashfull: false,
            tbhits: false,
            pv: false,
            timeleft: false,
            latency: false,
            min: false,
            match_line: String::new(),
        }
    }
}

impl PgnOut {
    /// The `-pgnout` key=value pairs after `file=`, leaving fastchess defaults unsaid.
    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.notation != "san" {
            args.push(format!("notation={}", self.notation));
        }
        for (key, on) in [
            ("nodes", self.nodes),
            ("seldepth", self.seldepth),
            ("nps", self.nps),
            ("hashfull", self.hashfull),
            ("tbhits", self.tbhits),
            ("pv", self.pv),
            ("timeleft", self.timeleft),
            ("latency", self.latency),
            ("min", self.min),
        ] {
            if on {
                args.push(format!("{key}=true"));
            }
        }
        if !self.match_line.trim().is_empty() {
            args.push(format!("match_line={}", self.match_line.trim()));
        }
        args
    }
}

/// The tournament built in the form. Text fields are kept as typed and checked by [`Setup::args`].
#[derive(Clone, Debug)]
pub struct Setup {
    pub participants: Vec<Participant>,
    pub limit: Limit,
    pub limit_value: String,
    pub hash: String,
    pub threads: String,
    pub rounds: String,
    pub games_per_round: u32,
    pub concurrency: String,
    pub gauntlet: bool,
    /// Fischer Random (Chess960) instead of standard chess.
    pub chess960: bool,
    /// Never swap colours between the games of a round.
    pub noswap: bool,
    /// Reverse the colours of the schedule.
    pub reverse: bool,
    pub openings: String,
    pub random_order: bool,
    pub plies: String,
    pub sprt: bool,
    pub elo0: String,
    pub elo1: String,
    pub alpha: String,
    pub beta: String,
    /// `normalized`, `logistic` or `bayesian`.
    pub sprt_model: &'static str,
    pub draw_adjudication: bool,
    /// Draw adjudication: from this full move, after this many moves in a row with both
    /// scores within this many centipawns of zero.
    pub draw_movenumber: String,
    pub draw_movecount: String,
    pub draw_score: String,
    pub resign_adjudication: bool,
    /// Resign adjudication: after this many moves in a row beyond this many centipawns,
    /// as seen by both engines when `resign_twosided`, else by the losing one.
    pub resign_movecount: String,
    pub resign_score: String,
    pub resign_twosided: bool,
    pub maxmoves: String,
    /// Syzygy folders for tablebase adjudication; empty turns it off.
    pub tb_paths: String,
    pub tb_pieces: String,
    pub tb_ignore50: bool,
    /// `BOTH`, `WIN_LOSS` or `DRAW`.
    pub tb_adjudicate: &'static str,
    pub recover: bool,
    pub seed: String,
    pub wait: String,
    pub time_margin: String,
    pub restart: bool,
    pub affinity: bool,
    /// Command-line arguments passed to every engine.
    pub engine_args: String,
    pub pgn: PgnOut,
    pub extra: String,
}

pub fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(2).max(1))
        .unwrap_or(1)
}

pub(super) fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T> {
    text.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{what} must be a number, not \"{}\"", text.trim()))
}

impl Setup {
    /// The fastchess arguments, writing games and the resumable config into `dir`.
    pub fn args(&self, dir: &Path) -> Result<Vec<String>> {
        ensure!(self.participants.len() >= 2, "Choose at least two engines");
        let mut args = self.engine_args();
        args.extend(self.each_args()?);
        args.extend(self.schedule_args()?);
        args.extend(self.opening_args()?);
        args.extend(self.adjudication_args()?);
        // Results after every game keep the table live.
        args.extend(["-ratinginterval".into(), "1".into()]);
        args.extend(split_args(&self.extra)?);
        args.extend(output_args(dir, &self.pgn));
        Ok(args)
    }

    /// One `-engine` per participant, with names made unique and empty options left out.
    fn engine_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let mut push = |items: &[String]| args.extend_from_slice(items);
        let mut names = std::collections::HashSet::new();
        for participant in &self.participants {
            let base = participant.name.trim();
            let base = if base.is_empty() { "engine" } else { base };
            // fastchess needs distinct names; the same engine may take part twice.
            let mut name = base.to_owned();
            let mut copy = 2;
            while !names.insert(name.clone()) {
                name = format!("{base} ({copy})");
                copy += 1;
            }
            let mut engine = vec![
                "-engine".to_owned(),
                format!("cmd={}", participant.cmd.display()),
                format!("name={name}"),
            ];
            if let Some(parent) = participant
                .cmd
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                engine.push(format!("dir={}", parent.display()));
            }
            for (option, value) in &participant.options {
                // fastchess rejects key=value arguments with an empty value.
                // Leave unset string options at the engine's own defaults.
                if !SKIPPED_OPTIONS.contains(&option.as_str()) && !value.trim().is_empty() {
                    engine.push(format!("option.{option}={value}"));
                }
            }
            push(&engine);
        }
        args
    }

    /// `-each`: the time limit, hash and threads every engine gets, and how it is run.
    fn each_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();
        let mut push = |items: &[String]| args.extend_from_slice(items);
        let value = self.limit_value.trim();
        ensure!(
            !value.is_empty(),
            "Enter a time limit: {}",
            self.limit.hint()
        );
        let limit = match self.limit {
            Limit::TimeControl => {
                ensure!(
                    value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'/' | b':')),
                    "Time control looks like 10+0.1, 40/60 or 1:00+1"
                );
                format!("tc={value}")
            }
            Limit::MoveTime => format!("st={}", number::<f64>(value, "Time per move")?),
            Limit::Nodes => format!("nodes={}", number::<u64>(value, "Nodes")?),
            Limit::Depth => format!("depth={}", number::<u32>(value, "Depth")?),
        };
        push(&[
            "-each".into(),
            limit,
            format!("option.Hash={}", number::<u32>(&self.hash, "Hash")?),
            format!(
                "option.Threads={}",
                number::<u32>(&self.threads, "Threads")?
            ),
        ]);
        if !self.time_margin.trim().is_empty() {
            push(&[format!(
                "timemargin={}",
                number::<u64>(&self.time_margin, "Time margin")?
            )]);
        }
        if self.restart {
            push(&["restart=on".into()]);
        }
        if !self.engine_args.trim().is_empty() {
            push(&[format!("args={}", self.engine_args.trim())]);
        }
        Ok(args)
    }

    /// Seed, rounds, concurrency and pairing.
    fn schedule_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();
        let mut push = |items: &[String]| args.extend_from_slice(items);
        if !self.seed.trim().is_empty() {
            push(&[
                "-srand".into(),
                number::<u64>(&self.seed, "Random seed")?.to_string(),
            ]);
        }
        if !self.wait.trim().is_empty() {
            push(&[
                "-wait".into(),
                number::<u64>(&self.wait, "Delay between games")?.to_string(),
            ]);
        }
        if self.affinity {
            push(&["-use-affinity".into()]);
        }
        let rounds = number::<u64>(&self.rounds, "Rounds")?;
        ensure!(rounds > 0, "Rounds must be at least 1");
        push(&["-rounds".into(), rounds.to_string()]);
        if self.games_per_round == 2 {
            push(&["-repeat".into()]);
        } else {
            push(&["-games".into(), "1".into()]);
        }
        let concurrency = number::<u32>(&self.concurrency, "Concurrency")?;
        ensure!(concurrency > 0, "Concurrency must be at least 1");
        push(&["-concurrency".into(), concurrency.to_string()]);
        if self.gauntlet {
            push(&["-tournament".into(), "gauntlet".into()]);
        }
        if self.chess960 {
            push(&["-variant".into(), "fischerandom".into()]);
        }
        if self.noswap {
            push(&["-noswap".into()]);
        }
        if self.reverse {
            push(&["-reverse".into()]);
        }
        Ok(args)
    }

    /// The opening book, its order and how many plies to play from it.
    fn opening_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();
        let mut push = |items: &[String]| args.extend_from_slice(items);
        let openings = self.openings.trim();
        if !openings.is_empty() {
            let lower = openings.to_lowercase();
            let format = if lower.ends_with(".pgn") {
                "pgn"
            } else if lower.ends_with(".epd") {
                "epd"
            } else {
                bail!("The opening book must be an .epd or .pgn file")
            };
            ensure!(
                Path::new(openings).is_file(),
                "Opening book not found: {openings}"
            );
            let mut opening = vec![
                "-openings".to_owned(),
                format!("file={openings}"),
                format!("format={format}"),
                format!(
                    "order={}",
                    if self.random_order {
                        "random"
                    } else {
                        "sequential"
                    }
                ),
            ];
            if !self.plies.trim().is_empty() {
                opening.push(format!(
                    "plies={}",
                    number::<u32>(&self.plies, "Opening plies")?
                ));
            }
            push(&opening);
        }
        Ok(args)
    }

    /// SPRT, draw and resign adjudication, the move cap, tablebases and recovery.
    fn adjudication_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();
        let mut push = |items: &[String]| args.extend_from_slice(items);
        if self.sprt {
            push(&[
                "-sprt".into(),
                format!("elo0={}", number::<f64>(&self.elo0, "elo0")?),
                format!("elo1={}", number::<f64>(&self.elo1, "elo1")?),
                format!("alpha={}", number::<f64>(&self.alpha, "alpha")?),
                format!("beta={}", number::<f64>(&self.beta, "beta")?),
                format!("model={}", self.sprt_model),
            ]);
        }
        if self.draw_adjudication {
            push(&[
                "-draw".into(),
                format!(
                    "movenumber={}",
                    number::<u32>(&self.draw_movenumber, "Draw adjudication's first move")?
                ),
                format!(
                    "movecount={}",
                    number::<u32>(&self.draw_movecount, "Draw adjudication's move count")?
                ),
                format!(
                    "score={}",
                    number::<u32>(&self.draw_score, "Draw adjudication's score")?
                ),
            ]);
        }
        if self.resign_adjudication {
            push(&[
                "-resign".into(),
                format!(
                    "movecount={}",
                    number::<u32>(&self.resign_movecount, "Resign adjudication's move count")?
                ),
                format!(
                    "score={}",
                    number::<u32>(&self.resign_score, "Resign adjudication's score")?
                ),
                format!("twosided={}", self.resign_twosided),
            ]);
        }
        if !self.maxmoves.trim().is_empty() {
            push(&[
                "-maxmoves".into(),
                number::<u32>(&self.maxmoves, "Maximum moves")?.to_string(),
            ]);
        }
        let tb_paths = self.tb_paths.trim();
        if !tb_paths.is_empty() {
            push(&["-tb".into(), tb_paths.to_owned()]);
            if !self.tb_pieces.trim().is_empty() {
                push(&[
                    "-tbpieces".into(),
                    number::<u32>(&self.tb_pieces, "Tablebase pieces")?.to_string(),
                ]);
            }
            if self.tb_ignore50 {
                push(&["-tbignore50".into()]);
            }
            if self.tb_adjudicate != "BOTH" {
                push(&["-tbadjudicate".into(), self.tb_adjudicate.into()]);
            }
        }
        if self.recover {
            push(&["-recover".into()]);
        }
        Ok(args)
    }
}

/// Games and the resumable config go into the tournament's folder.
pub(super) fn output_args(dir: &Path, pgn: &PgnOut) -> Vec<String> {
    let mut args = vec![
        "-pgnout".into(),
        format!("file={}", dir.join("games.pgn").display()),
    ];
    args.extend(pgn.args());
    args.extend([
        "-config".into(),
        format!("outname={}", dir.join("config.json").display()),
    ]);
    args
}

/// Arguments that continue the tournament saved in `config`.
pub fn resume_args(config: &Path) -> Vec<String> {
    vec![
        "-config".into(),
        format!("file={}", config.display()),
        format!("outname={}", config.display()),
    ]
}
