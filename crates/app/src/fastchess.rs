//! Engine tournaments through fastchess, which is downloaded on first use rather than bundled.
//!
//! Fast builds a fastchess command line (or takes a pasted one, or a saved `config.json` to
//! continue), runs it in a folder of its own and reads its human-readable output. See
//! `docs/fastchess-integration.md` for what fastchess could offer to make this less fragile.
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context as _, Result, bail, ensure};

use crate::engine_catalog::{Download, Entry, Format};

pub const VERSION: &str = "1.8.3-alpha";

/// The pinned fastchess release, checked against the SHA-256 digests GitHub reports.
pub fn release() -> Entry {
    let asset = |os: &str, arch: &str, name: &str, sha256: &str| {
        let windows = os == "windows";
        Download {
            os: os.into(),
            arch: arch.into(),
            url: format!(
                "https://github.com/Disservin/fastchess/releases/download/v{VERSION}/fastchess-{name}.{}",
                if windows { "zip" } else { "tar" }
            ),
            sha256: sha256.into(),
            format: if windows { Format::Zip } else { Format::Tar },
            executable: format!(
                "fastchess-{name}/fastchess{}",
                if windows { ".exe" } else { "" }
            ),
        }
    };
    Entry {
        id: "fastchess".into(),
        name: "fastchess".into(),
        version: VERSION.into(),
        description: "Command-line tournament manager for UCI engines".into(),
        homepage: "https://github.com/Disservin/fastchess".into(),
        source_url: format!("https://github.com/Disservin/fastchess/tree/v{VERSION}"),
        license: "MIT".into(),
        license_url: format!(
            "https://raw.githubusercontent.com/Disservin/fastchess/v{VERSION}/LICENSE"
        ),
        protocol: "tool".into(),
        defaults: BTreeMap::new(),
        downloads: vec![
            asset(
                "linux",
                "x86_64",
                "linux-x86-64",
                "7ab4879eda78de88f210d87c04c65f9ea274d20e18d4f1745c6a3ddd8ef35d7e",
            ),
            asset(
                "linux",
                "aarch64",
                "linux-arm64",
                "93a3556493e50b63f8bdab1300135792eff71c0cebb45a1f086dfc57502601af",
            ),
            asset(
                "macos",
                "x86_64",
                "mac-x86-64",
                "2a9553621333133536154d07efa9d143a46c211755e760296493ee8d4f2810af",
            ),
            asset(
                "macos",
                "aarch64",
                "mac-arm64",
                "f7a812354d5c5944c0c8070c6fc492077589f98560412a977bb077cee4f441d8",
            ),
            asset(
                "windows",
                "x86_64",
                "windows-x86-64",
                "e4c66cceec4122d0638022da55e6841487a1dd9a4819685893749dbd7c645033",
            ),
            asset(
                "windows",
                "aarch64",
                "windows-arm64",
                "76d74706ac47808d3e4d2197434976e86e029f2def7265095c58ae7aa3845333",
            ),
        ],
    }
}

fn tools_dir(data: &Path) -> PathBuf {
    data.join("tools")
}

/// The downloaded executable, when a complete installation of the pinned version exists.
pub fn installed(data: &Path) -> Option<PathBuf> {
    let entry = release();
    let asset = entry.native_download()?;
    let dir = entry.install_dir_in(&tools_dir(data), asset);
    let executable = dir.join(&asset.executable);
    (dir.join("fast-install.json").is_file() && executable.is_file()).then_some(executable)
}

pub fn install(data: &Path, cancel: Arc<AtomicBool>, progress: impl Fn(String)) -> Result<PathBuf> {
    crate::engine_catalog::install_into(&release(), &tools_dir(data), cancel, progress)
}

/// Where each tournament keeps its config, games and output.
pub fn tournaments_dir(data: &Path) -> PathBuf {
    data.join("tournaments")
}

/// Bytes used by the files under `path`, without following symlinks.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

/// Sizes like 512 B, 3.4 MB or 1.2 GB.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    while value >= 1000. && unit + 1 < UNITS.len() {
        value /= 1000.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Deletes every tournament folder except `keep`, and returns how many went and the bytes
/// freed. Stops at the first folder it cannot delete.
pub fn clear_tournaments(data: &Path, keep: Option<&Path>) -> Result<(usize, u64)> {
    let root = tournaments_dir(data);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok((0, 0));
    };
    let (mut count, mut freed) = (0, 0);
    for entry in entries.flatten() {
        let path = entry.path();
        if Some(path.as_path()) == keep {
            continue;
        }
        let size;
        if entry.file_type()?.is_dir() {
            size = dir_size(&path);
            std::fs::remove_dir_all(&path)?;
        } else {
            size = entry.metadata().map_or(0, |meta| meta.len());
            std::fs::remove_file(&path)?;
        }
        count += 1;
        freed += size;
    }
    Ok((count, freed))
}

/// A new, empty folder for one tournament.
pub fn new_run_dir(data: &Path) -> Result<PathBuf> {
    let root = tournaments_dir(data);
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    for suffix in 0..1000 {
        let dir = root.join(if suffix == 0 {
            format!("t{stamp}")
        } else {
            format!("t{stamp}-{suffix}")
        });
        if std::fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    bail!("Could not create a tournament folder in {}", root.display())
}

/// One engine in a tournament.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Participant {
    pub name: String,
    pub cmd: PathBuf,
    pub options: BTreeMap<String, String>,
}

/// Options Fast saves for analysis that would distort a match, or are set for every engine.
const SKIPPED_OPTIONS: [&str; 5] = ["Hash", "Threads", "MultiPV", "UCI_AnalyseMode", "Ponder"];

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

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T> {
    text.trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{what} must be a number, not \"{}\"", text.trim()))
}

impl Setup {
    /// The fastchess arguments, writing games and the resumable config into `dir`.
    pub fn args(&self, dir: &Path) -> Result<Vec<String>> {
        ensure!(self.participants.len() >= 2, "Choose at least two engines");
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
        // Results after every game keep the table live.
        push(&["-ratinginterval".into(), "1".into()]);
        push(&split_args(&self.extra)?);
        args.extend(output_args(dir, &self.pgn));
        Ok(args)
    }
}

/// Games and the resumable config go into the tournament's folder.
fn output_args(dir: &Path, pgn: &PgnOut) -> Vec<String> {
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

/// What the user pasted to start from.
#[derive(Clone, Debug, PartialEq)]
pub enum Pasted {
    /// A `config.json` fastchess saved, to continue.
    Config(serde_json::Value),
    /// A fastchess command line.
    Args(Vec<String>),
}

pub fn parse_paste(text: &str) -> Result<Pasted> {
    let text = text.trim();
    ensure!(
        !text.is_empty(),
        "Paste a fastchess config.json or command line"
    );
    if text.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(text).context("The pasted config is not valid JSON")?;
        let engines = value
            .get("engines")
            .and_then(|engines| engines.as_array())
            .context("The pasted config has no \"engines\" list; is it a fastchess config.json?")?;
        ensure!(
            engines.len() >= 2,
            "The pasted config has fewer than two engines"
        );
        return Ok(Pasted::Config(value));
    }
    let mut args = split_args(text)?;
    // A copied command usually starts with the program itself.
    if args.first().is_some_and(|first| {
        !first.starts_with('-')
            && Path::new(first)
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("fastchess"))
    }) {
        args.remove(0);
    }
    ensure!(
        args.iter()
            .any(|arg| arg == "-engine" || arg == "-config" || arg == "-quick"),
        "The pasted command has no -engine, -config or -quick option"
    );
    Ok(Pasted::Args(args))
}

impl Pasted {
    /// Arguments to run, with the config saved into `dir` unless the paste names its own,
    /// and where that config will be.
    pub fn launch(&self, dir: &Path, cwd: &Path) -> Result<(Vec<String>, PathBuf)> {
        match self {
            Self::Config(value) => {
                let path = dir.join("config.json");
                std::fs::write(&path, serde_json::to_vec_pretty(value)?)
                    .context("Saving the pasted config")?;
                Ok((resume_args(&path), path))
            }
            Self::Args(args) => {
                let mut args = args.clone();
                let named = config_outname(&args).map(|name| cwd.join(name));
                let config = match named {
                    Some(path) => path,
                    None => {
                        let path = dir.join("config.json");
                        args.extend(["-config".into(), format!("outname={}", path.display())]);
                        path
                    }
                };
                Ok((args, config))
            }
        }
    }
}

/// The `outname=` of a `-config` option, if the command sets one.
fn config_outname(args: &[String]) -> Option<String> {
    let mut in_config = false;
    let mut found = None;
    for arg in args {
        if arg.starts_with('-') {
            in_config = arg == "-config";
        } else if in_config && let Some(name) = arg.strip_prefix("outname=") {
            found = Some(name.to_owned());
        }
    }
    found
}

/// Splits a command line the way a shell would, for the forms people paste: quotes,
/// backslash and `^` line continuations. Backslashes before other characters stay, so
/// Windows paths survive.
pub fn split_args(text: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                current.push(chars.next().unwrap_or('\\'));
            }
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\' | '^') if matches!(chars.peek(), Some('\n' | '\r')) => {
                while matches!(chars.peek(), Some('\n' | '\r')) {
                    chars.next();
                }
            }
            (None, '\\') if matches!(chars.peek(), Some(' ' | '"' | '\'' | '\\')) => {
                current.push(chars.next().unwrap_or('\\'));
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    ensure!(quote.is_none(), "Unclosed quote in the command line");
    if started {
        args.push(current);
    }
    Ok(args)
}

/// Quotes arguments for display, so the shown command can be pasted into a shell.
pub fn display_command(program: &str, args: &[String]) -> String {
    let quote = |arg: &str| {
        if !arg.is_empty()
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_=./+:,@%".contains(c))
        {
            arg.to_owned()
        } else {
            format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
        }
    };
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A line of fastchess output that Fast understands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Started {
        game: u64,
        total: u64,
    },
    Finished {
        game: u64,
        white: &'a str,
        black: &'a str,
        result: &'a str,
        reason: &'a str,
    },
    /// The dashes around each results block.
    Rule,
    Text(&'a str),
}

pub fn parse_line(line: &str) -> Line<'_> {
    let parsed = (|| {
        if line.len() >= 20 && line.bytes().all(|b| b == b'-') {
            return Some(Line::Rule);
        }
        if let Some(rest) = line.strip_prefix("Started game ") {
            let (game, rest) = rest.split_once(" of ")?;
            let (total, _) = rest.split_once(' ')?;
            return Some(Line::Started {
                game: game.parse().ok()?,
                total: total.parse().ok()?,
            });
        }
        let rest = line.strip_prefix("Finished game ")?;
        let (game, rest) = rest.split_once(" (")?;
        let (players, outcome) = rest.split_once("): ")?;
        let (white, black) = players.split_once(" vs ")?;
        let (result, reason) = match outcome.split_once(" {") {
            Some((result, reason)) => (result, reason.trim_end_matches('}')),
            None => (outcome, ""),
        };
        Some(Line::Finished {
            game: game.parse().ok()?,
            white,
            black,
            result,
            reason,
        })
    })();
    parsed.unwrap_or(Line::Text(line))
}

/// A head-to-head results block as numbers, for the stat tiles. Scores are from the first
/// engine's side, as fastchess prints them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    /// Engine A and engine B, from the block's heading.
    pub engines: Option<(String, String)>,
    pub elo: f64,
    pub elo_err: f64,
    pub nelo: f64,
    pub nelo_err: f64,
    pub los: f64,
    pub draw_ratio: f64,
    pub pairs_ratio: f64,
    pub wl_dd_ratio: f64,
    pub games: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub points: f64,
    pub score_pct: f64,
    /// Pentanomial counts from 0-2 to 2-0 points, for the pairs of games.
    pub ptnml: [u64; 5],
    /// Sequential test state, present only with SPRT.
    pub llr: Option<Llr>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Llr {
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Parses the numbers of a head-to-head block. `None` when the block is not one, such as
/// the table fastchess prints for more than two engines.
pub fn results(block: &[String]) -> Option<Results> {
    let mut out = Results::default();
    let (mut elo, mut los, mut games, mut ptnml, mut llr) = (false, false, false, false, None);
    for line in block {
        let line = line.trim();
        // The first number after `key` (ignoring commas and units).
        let number = |key: &str| -> Option<f64> {
            let rest = line.split_once(key)?.1;
            rest.split(|c: char| c == ',' || c.is_whitespace())
                .find(|token| !token.is_empty())?
                .parse()
                .ok()
        };
        if let Some(rest) = line.strip_prefix("Results of ") {
            let names = rest.trim_end_matches(':');
            out.engines = names.split_once(" vs ").map(|(a, b)| {
                let b = b.split(" (").next().unwrap_or(b);
                (
                    a.trim().to_owned(),
                    b.trim().trim_end_matches(':').to_owned(),
                )
            });
        } else if line.starts_with("Elo:") {
            // Elo: -58.45 +/- 98.05, nElo: -173.72 +/- 278.00
            let nums: Vec<f64> = line
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|token| token.parse().ok())
                .collect();
            if let [e, ee, n, ne] = nums[..] {
                (out.elo, out.elo_err, out.nelo, out.nelo_err) = (e, ee, n, ne);
                elo = true;
            }
        } else if line.starts_with("LOS:") {
            out.los = number("LOS:").unwrap_or_default();
            out.draw_ratio = number("DrawRatio:").unwrap_or_default();
            out.pairs_ratio = number("PairsRatio:").unwrap_or_default();
            los = true;
        } else if line.starts_with("Games:") {
            out.games = number("Games:").unwrap_or_default() as u64;
            out.wins = number("Wins:").unwrap_or_default() as u64;
            out.losses = number("Losses:").unwrap_or_default() as u64;
            out.draws = number("Draws:").unwrap_or_default() as u64;
            out.points = number("Points:").unwrap_or_default();
            out.score_pct = line
                .split_once('(')
                .and_then(|(_, rest)| rest.split('%').next())
                .and_then(|pct| pct.trim().parse().ok())
                .unwrap_or_default();
            games = true;
        } else if line.starts_with("Ptnml(0-2):") {
            let inside = line
                .split_once('[')
                .and_then(|(_, rest)| rest.split_once(']'));
            if let Some((list, _)) = inside {
                let values: Vec<u64> = list
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                if let Ok(values) = <[u64; 5]>::try_from(values) {
                    out.ptnml = values;
                    ptnml = true;
                }
            }
            out.wl_dd_ratio = number("Ratio:").unwrap_or_default();
        } else if line.starts_with("LLR:") {
            // LLR: -0.03 (-0.9%) (-2.94, 2.94) [0.00, 5.00]
            let value = number("LLR:");
            let bounds = line.rfind('(').and_then(|open| {
                let inner = line[open + 1..].split(')').next()?;
                let (lower, upper) = inner.split_once(',')?;
                Some((lower.trim().parse().ok()?, upper.trim().parse().ok()?))
            });
            if let (Some(value), Some((lower, upper))) = (value, bounds) {
                llr = Some(Llr {
                    value,
                    lower,
                    upper,
                });
            }
        }
    }
    (elo && los && games && ptnml).then(|| {
        out.llr = llr;
        out
    })
}

/// A running fastchess process. Output lines arrive on `lines`; the channel closes when
/// the process has exited and its output is read.
pub struct Process {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
    /// The child is the guard of [`crate::process_guard`], not fastchess itself.
    #[cfg(unix)]
    guarded: bool,
    /// The guard's stdin. Closing it, as happens when Fast dies, stops fastchess.
    _guard_input: Option<std::process::ChildStdin>,
    #[cfg(windows)]
    job: Option<Arc<crate::process_guard::Job>>,
    pub lines: smol::channel::Receiver<String>,
}

#[derive(Clone)]
pub struct StopHandle {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
    #[cfg(unix)]
    guarded: bool,
    #[cfg(windows)]
    job: Option<Arc<crate::process_guard::Job>>,
}

/// Starts fastchess, set up to stop when Fast does (see [`crate::process_guard`]). Event
/// lines go to `live` instead of `lines` when it is given.
pub fn spawn(
    program: &Path,
    args: &[String],
    cwd: &Path,
    live: Option<Arc<Mutex<crate::tournament_live::LiveGames>>>,
) -> Result<Process> {
    let start = |guard: bool| {
        let (mut command, guarded) = if guard {
            crate::process_guard::command(program)
        } else {
            (Command::new(program), false)
        };
        command
            .args(args)
            .current_dir(cwd)
            .stdin(if guarded {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // fastchess still gets a (hidden) console, which the engines it starts share.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.spawn().map(|child| (child, guarded))
    };
    let (mut child, _guarded) = start(true)
        .or_else(|err| {
            log::warn!("could not start fastchess under a guard ({err}); starting it directly");
            start(false)
        })
        .with_context(|| format!("Starting {}", program.display()))?;
    let guard_input = child.stdin.take();
    #[cfg(windows)]
    let job = match crate::process_guard::Job::attach(&child) {
        Ok(job) => Some(Arc::new(job)),
        Err(err) => {
            log::warn!("fastchess will not stop with Fast: {err}");
            None
        }
    };
    let (sender, lines) = smol::channel::bounded::<String>(4096);
    let stdout = child
        .stdout
        .take()
        .map(|out| Box::new(out) as Box<dyn Read + Send>);
    let stderr = child
        .stderr
        .take()
        .map(|err| Box::new(err) as Box<dyn Read + Send>);
    for pipe in [stdout, stderr].into_iter().flatten() {
        let sender = sender.clone();
        let live = live.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut buffer = Vec::new();
            loop {
                buffer.clear();
                // Engines may print anything; a stray byte must not stop the reading,
                // or fastchess would block on a full pipe.
                match reader.read_until(b'\n', &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&buffer);
                let line = line.trim_end_matches(['\n', '\r']);
                if let Some(live) = &live
                    && crate::tournament_live::is_event(line)
                {
                    if let Ok(mut live) = live.lock() {
                        live.feed(line);
                    }
                    continue;
                }
                if sender.send_blocking(line.to_owned()).is_err() {
                    break;
                }
            }
        });
    }
    drop(sender);
    Ok(Process {
        child: Arc::new(Mutex::new(child)),
        stopping: Arc::new(AtomicBool::new(false)),
        #[cfg(unix)]
        guarded: _guarded,
        _guard_input: guard_input,
        #[cfg(windows)]
        job,
        lines,
    })
}

impl Process {
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle {
            child: self.child.clone(),
            stopping: self.stopping.clone(),
            #[cfg(unix)]
            guarded: self.guarded,
            #[cfg(windows)]
            job: self.job.clone(),
        }
    }

    /// Blocks until the process exits, once its output is drained. Describes how it ended.
    pub fn wait(&self) -> Exit {
        let status = loop {
            match self.child.lock().map(|mut child| child.try_wait()) {
                Ok(Ok(Some(status))) => break Some(status),
                Ok(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(50)),
                _ => break None,
            }
        };
        Exit {
            success: status.is_some_and(|status| status.success()),
            stopped: self.stopping.load(Ordering::Relaxed),
            code: status.and_then(|status| status.code()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Exit {
    pub success: bool,
    pub stopped: bool,
    pub code: Option<i32>,
}

impl StopHandle {
    /// Asks fastchess to stop as on Ctrl+C, so it saves the config for resuming. A second
    /// call kills it outright. Windows has no signal to send, so there it is always killed,
    /// engines included.
    pub fn stop(&self) {
        let again = self.stopping.swap(true, Ordering::Relaxed);
        let Ok(mut child) = self.child.lock() else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        {
            let Ok(pid) = i32::try_from(child.id()) else {
                return;
            };
            // The guard passes SIGINT on to fastchess and kills it on SIGUSR2; killing the
            // guard itself would leave fastchess running.
            let signal = match (again, self.guarded) {
                (false, _) => libc::SIGINT,
                (true, true) => libc::SIGUSR2,
                (true, false) => libc::SIGKILL,
            };
            // SAFETY: plain signal to a child we own and have not reaped.
            unsafe {
                libc::kill(pid, signal);
            }
        }
        #[cfg(windows)]
        {
            let _ = again;
            match &self.job {
                Some(job) => job.terminate(),
                None => {
                    let _ = child.kill();
                }
            }
        }
    }
}

/// A tournament found on disk that can be continued.
#[derive(Clone, Debug)]
pub struct SavedRun {
    pub dir: PathBuf,
    pub config: PathBuf,
    pub engines: Vec<String>,
    pub games: u64,
    pub modified: std::time::SystemTime,
}

/// Games recorded in a saved config's results, 0 when there is none.
pub fn saved_games(config: &Path) -> u64 {
    std::fs::read_to_string(config)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map_or(0, |value| games_in(&value))
}

fn games_in(value: &serde_json::Value) -> u64 {
    value
        .get("stats")
        .and_then(|stats| stats.as_object())
        .map(|stats| {
            stats
                .values()
                .map(|pair| {
                    ["wins", "losses", "draws"]
                        .iter()
                        .filter_map(|key| pair.get(*key)?.as_u64())
                        .sum::<u64>()
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Tournaments with a saved config, newest first.
pub fn saved_runs(data: &Path) -> Vec<SavedRun> {
    let Ok(entries) = std::fs::read_dir(tournaments_dir(data)) else {
        return Vec::new();
    };
    let mut runs: Vec<SavedRun> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let config = dir.join("config.json");
            let modified = std::fs::metadata(&config).ok()?.modified().ok()?;
            let text = std::fs::read_to_string(&config).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let engines = value
                .get("engines")?
                .as_array()?
                .iter()
                .filter_map(|engine| engine.get("name")?.as_str().map(str::to_owned))
                .collect();
            let games = games_in(&value);
            Some(SavedRun {
                dir,
                config,
                engines,
                games,
                modified,
            })
        })
        .collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.modified));
    runs
}

/// The game fastchess tagged `[GameId "id"]`, which builds with `-events` write.
pub fn game_by_id(pgn: &str, id: u64) -> Option<String> {
    let tag = format!("[GameId \"{id}\"]");
    (0..count_games(pgn))
        .filter_map(|index| nth_game(pgn, index))
        .find(|game| game.lines().any(|line| line.trim() == tag))
}

/// The `index`-th game (from 0) of a PGN file with several games.
pub fn nth_game(pgn: &str, index: usize) -> Option<String> {
    let mut starts: Vec<usize> = pgn
        .match_indices("[Event ")
        .map(|(at, _)| at)
        .filter(|&at| at == 0 || pgn[..at].ends_with('\n'))
        .collect();
    let start = *starts.get(index)?;
    starts.push(pgn.len());
    let end = starts[index + 1];
    Some(pgn[start..end].trim().to_owned())
}

pub fn count_games(pgn: &str) -> usize {
    pgn.match_indices("[Event ")
        .filter(|&(at, _)| at == 0 || pgn[..at].ends_with('\n'))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_tournament_folders_but_the_kept_one() {
        let data = tempfile::tempdir().unwrap();
        let root = tournaments_dir(data.path());
        for name in ["a", "b"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("games.pgn"), "1234").unwrap();
        }
        assert_eq!(dir_size(&root), 8);
        let kept = root.join("b");
        assert_eq!(clear_tournaments(data.path(), Some(&kept)).unwrap(), (1, 4));
        assert!(!root.join("a").exists());
        assert!(kept.join("games.pgn").exists());
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_300_000), "1.3 MB");
    }

    fn setup(participants: Vec<Participant>) -> Setup {
        Setup {
            participants,
            limit: Limit::TimeControl,
            limit_value: "10+0.1".into(),
            hash: "16".into(),
            threads: "1".into(),
            rounds: "100".into(),
            games_per_round: 2,
            concurrency: "4".into(),
            gauntlet: false,
            chess960: false,
            noswap: false,
            reverse: false,
            openings: String::new(),
            random_order: true,
            plies: String::new(),
            sprt: true,
            elo0: "0".into(),
            elo1: "2".into(),
            alpha: "0.05".into(),
            beta: "0.05".into(),
            sprt_model: "normalized",
            draw_adjudication: false,
            draw_movenumber: "32".into(),
            draw_movecount: "6".into(),
            draw_score: "10".into(),
            resign_adjudication: false,
            resign_movecount: "3".into(),
            resign_score: "600".into(),
            resign_twosided: true,
            maxmoves: String::new(),
            tb_paths: String::new(),
            tb_pieces: String::new(),
            tb_ignore50: false,
            tb_adjudicate: "BOTH",
            recover: true,
            seed: String::new(),
            wait: String::new(),
            time_margin: String::new(),
            restart: false,
            affinity: false,
            engine_args: String::new(),
            pgn: PgnOut::default(),
            extra: "-srand 42".into(),
        }
    }

    #[test]
    fn variant_tablebases_and_pgn_options() {
        let mut setup = setup(vec![engine("A"), engine("B")]);
        setup.chess960 = true;
        setup.noswap = true;
        setup.engine_args = "--uci --quiet".into();
        setup.tb_paths = "/tb/345".into();
        setup.tb_pieces = "5".into();
        setup.tb_ignore50 = true;
        setup.tb_adjudicate = "DRAW";
        setup.pgn.notation = "uci";
        setup.pgn.nodes = true;
        setup.pgn.pv = true;
        let args = setup.args(Path::new("/out")).unwrap();
        let after = |flag: &str| {
            let at = args.iter().position(|arg| arg == flag).unwrap();
            args[at + 1].clone()
        };
        assert_eq!(after("-variant"), "fischerandom");
        assert!(args.contains(&"-noswap".to_owned()));
        assert!(!args.contains(&"-reverse".to_owned()));
        assert!(args.contains(&"args=--uci --quiet".to_owned()));
        assert_eq!(after("-tb"), "/tb/345");
        assert_eq!(after("-tbpieces"), "5");
        assert!(args.contains(&"-tbignore50".to_owned()));
        assert_eq!(after("-tbadjudicate"), "DRAW");
        assert!(args.contains(&"model=normalized".to_owned()));
        let pgnout = args.iter().position(|arg| arg == "-pgnout").unwrap();
        assert_eq!(
            args[pgnout + 2..pgnout + 5],
            ["notation=uci", "nodes=true", "pv=true"]
        );
    }

    #[test]
    fn advanced_engine_and_scheduling_options() {
        let mut setup = setup(vec![engine("A"), engine("B")]);
        setup.extra.clear();
        setup.seed = "42".into();
        setup.wait = "100".into();
        setup.time_margin = "50".into();
        setup.restart = true;
        setup.affinity = true;
        let args = setup.args(Path::new("/tmp/run")).unwrap();
        for expected in ["timemargin=50", "restart=on", "-use-affinity"] {
            assert!(args.iter().any(|arg| arg == expected));
        }
        for pair in [["-srand", "42"], ["-wait", "100"]] {
            assert!(args.windows(2).any(|args| args == pair));
        }
        let each = args.iter().position(|arg| arg == "-each").unwrap();
        let next_flag = args[each + 1..]
            .iter()
            .position(|arg| arg.starts_with('-'))
            .unwrap()
            + each
            + 1;
        assert!(args[each..next_flag].iter().any(|arg| arg == "restart=on"));
        setup.wait = "-1".into();
        assert!(setup.args(Path::new("/tmp/run")).is_err());
        setup.wait.clear();
        setup.seed = "invalid".into();
        assert!(setup.args(Path::new("/tmp/run")).is_err());
    }

    fn engine(name: &str) -> Participant {
        Participant {
            name: name.into(),
            cmd: PathBuf::from(format!("/engines/{name}")),
            options: [
                ("Hash".to_owned(), "512".to_owned()),
                ("EvalFile".to_owned(), "nn.nnue".to_owned()),
            ]
            .into(),
        }
    }

    #[test]
    fn release_has_a_build_for_every_platform_fast_ships() {
        let entry = release();
        for (os, arch) in [
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("windows", "x86_64"),
            ("windows", "aarch64"),
        ] {
            let asset = entry
                .downloads
                .iter()
                .find(|asset| asset.os == os && asset.arch == arch)
                .unwrap();
            assert!(
                asset
                    .url
                    .starts_with("https://github.com/Disservin/fastchess/")
            );
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.executable.starts_with("fastchess-"));
        }
    }

    #[test]
    fn registered_engine_omits_empty_options_and_preserves_configured_values() {
        let mut registered = engine("sf");
        registered.options.extend([
            ("Debug Log File".into(), "".into()),
            ("SyzygyPath".into(), "   ".into()),
            ("Skill Level".into(), "0".into()),
            ("UCI_ShowWDL".into(), "false".into()),
            ("Some File".into(), "/books/path with spaces/file".into()),
        ]);
        let args = setup(vec![registered, engine("other")])
            .args(Path::new("/tmp/run"))
            .unwrap();
        assert!(
            !args
                .iter()
                .any(|arg| arg.starts_with("option.Debug Log File=")
                    || arg.starts_with("option.SyzygyPath="))
        );
        assert!(args.iter().any(|arg| arg == "option.Skill Level=0"));
        assert!(args.iter().any(|arg| arg == "option.UCI_ShowWDL=false"));
        assert!(
            args.iter()
                .any(|arg| arg == "option.Some File=/books/path with spaces/file")
        );
        assert!(args.iter().any(|arg| arg == "option.EvalFile=nn.nnue"));
    }

    #[test]
    fn setup_builds_a_complete_command_with_unique_names() {
        let dir = Path::new("/data/t1");
        let args = setup(vec![engine("sf"), engine("sf")]).args(dir).unwrap();
        let text = args.join(" ");
        assert!(
            text.contains("-engine cmd=/engines/sf name=sf dir=/engines option.EvalFile=nn.nnue")
        );
        assert!(text.contains("name=sf (2)"));
        assert!(!text.contains("option.Hash=512"), "{text}");
        assert!(text.contains("-each tc=10+0.1 option.Hash=16 option.Threads=1"));
        assert!(text.contains("-rounds 100 -repeat -concurrency 4"));
        assert!(text.contains("-sprt elo0=0 elo1=2 alpha=0.05 beta=0.05"));
        assert!(text.contains("-recover"));
        assert!(text.contains("-srand 42"));
        assert!(text.ends_with(&format!(
            "-pgnout file={} -config outname={}",
            dir.join("games.pgn").display(),
            dir.join("config.json").display()
        )));
    }

    #[test]
    fn setup_rejects_bad_input() {
        let dir = Path::new("/data/t1");
        assert!(setup(vec![engine("a")]).args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.rounds = "many".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.limit_value = "10 minutes".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.openings = "/no/such/book.epd".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.draw_adjudication = true;
        bad.draw_score = "-5".into();
        assert!(bad.args(dir).is_err());
    }

    #[test]
    fn setup_passes_the_adjudication_rules_given() {
        let dir = Path::new("/data/t1");
        let mut setup = setup(vec![engine("a"), engine("b")]);
        setup.draw_adjudication = true;
        (
            setup.draw_movenumber,
            setup.draw_movecount,
            setup.draw_score,
        ) = ("40".into(), "8".into(), "5".into());
        setup.resign_adjudication = true;
        (setup.resign_movecount, setup.resign_score) = ("4".into(), "800".into());
        setup.resign_twosided = false;
        let args = setup.args(dir).unwrap();
        let after = |flag: &str| {
            let start = args.iter().position(|arg| arg == flag).unwrap() + 1;
            args[start..start + 3].join(" ")
        };
        assert_eq!(after("-draw"), "movenumber=40 movecount=8 score=5");
        assert_eq!(after("-resign"), "movecount=4 score=800 twosided=false");
    }

    #[test]
    fn splits_pasted_command_lines() {
        assert_eq!(
            split_args("./fastchess -engine cmd=\"my engine\" name='a b' \\\n  -each tc=8+0.08")
                .unwrap(),
            [
                "./fastchess",
                "-engine",
                "cmd=my engine",
                "name=a b",
                "-each",
                "tc=8+0.08"
            ]
        );
        assert_eq!(
            split_args(r"-engine cmd=C:\engines\sf.exe ^").unwrap(),
            ["-engine", r"cmd=C:\engines\sf.exe", "^"]
        );
        assert!(split_args("-engine cmd=\"open").is_err());
    }

    #[test]
    fn pasted_commands_and_configs_are_recognised() {
        let Pasted::Args(args) =
            parse_paste("fastchess.exe -engine cmd=a -engine cmd=b -rounds 2").unwrap()
        else {
            panic!("not a command");
        };
        assert_eq!(args[0], "-engine");
        let dir = tempfile::tempdir().unwrap();
        let (launch, config) = Pasted::Args(args.clone())
            .launch(dir.path(), dir.path())
            .unwrap();
        assert_eq!(config, dir.path().join("config.json"));
        assert_eq!(launch[launch.len() - 2], "-config");
        let own = [
            args.clone(),
            vec!["-config".into(), "outname=mine.json".into()],
        ]
        .concat();
        let (launch, config) = Pasted::Args(own.clone())
            .launch(dir.path(), Path::new("/work"))
            .unwrap();
        assert_eq!(launch, own);
        assert_eq!(config, Path::new("/work/mine.json"));

        let json = r#"{"engines": [{"name": "a"}, {"name": "b"}], "stats": {}}"#;
        let pasted = parse_paste(json).unwrap();
        let (launch, config) = pasted.launch(dir.path(), dir.path()).unwrap();
        assert!(config.is_file());
        assert_eq!(launch, resume_args(&config));

        assert!(parse_paste("{\"rounds\": 3}").is_err());
        assert!(parse_paste("hello world").is_err());
    }

    #[test]
    fn parses_fastchess_output() {
        assert_eq!(
            parse_line("Started game 4 of 300 (c vs a)"),
            Line::Started {
                game: 4,
                total: 300
            }
        );
        assert_eq!(
            parse_line("Finished game 3 (a vs c): 1-0 {White mates}"),
            Line::Finished {
                game: 3,
                white: "a",
                black: "c",
                result: "1-0",
                reason: "White mates"
            }
        );
        assert_eq!(
            parse_line("--------------------------------------------------"),
            Line::Rule
        );
        assert_eq!(parse_line("Finished match"), Line::Text("Finished match"));
        let block: Vec<String> = [
            "Results of a vs b (1+0.01, 1t, 8MB):",
            "Elo: -58.45 +/- 98.05, nElo: -173.72 +/- 278.00",
            "LOS: 11.03 %, DrawRatio: 66.67 %, PairsRatio: 0.00",
            "Games: 6, Wins: 0, Losses: 1, Draws: 5, Points: 2.5 (41.67 %)",
            "Ptnml(0-2): [0, 1, 2, 0, 0], WL/DD Ratio: 0.00",
            "LLR: -0.03 (-0.9%) (-2.94, 2.94) [0.00, 5.00]",
        ]
        .map(String::from)
        .into();

        let numbers = results(&block).expect("a head-to-head block");
        assert_eq!(numbers.engines, Some(("a".into(), "b".into())));
        assert_eq!((numbers.elo, numbers.elo_err), (-58.45, 98.05));
        assert_eq!((numbers.nelo, numbers.nelo_err), (-173.72, 278.0));
        assert_eq!((numbers.los, numbers.draw_ratio), (11.03, 66.67));
        assert_eq!(
            (numbers.games, numbers.wins, numbers.losses, numbers.draws),
            (6, 0, 1, 5)
        );
        assert_eq!((numbers.points, numbers.score_pct), (2.5, 41.67));
        assert_eq!(numbers.ptnml, [0, 1, 2, 0, 0]);
        assert_eq!(
            numbers.llr,
            Some(Llr {
                value: -0.03,
                lower: -2.94,
                upper: 2.94
            })
        );
        // A table for more engines is not a head-to-head block.
        assert!(results(&["Results of 3 engines:".to_owned()]).is_none());
    }

    #[test]
    fn finds_games_in_a_pgn_file() {
        let pgn = "[Event \"x\"]\n[Round \"2\"]\n\n1. e4 1-0\n\n[Event \"x\"]\n[Round \"1\"]\n\n1. d4 0-1\n";
        assert_eq!(count_games(pgn), 2);
        assert!(nth_game(pgn, 1).unwrap().ends_with("1. d4 0-1"));
        assert!(nth_game(pgn, 2).is_none());
    }

    #[test]
    fn saved_runs_report_engines_and_games() {
        let dir = tempfile::tempdir().unwrap();
        let run = tournaments_dir(dir.path()).join("t1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("config.json"),
            r#"{"engines": [{"name": "a"}, {"name": "b"}],
                "stats": {"a vs b": {"wins": 3, "losses": 1, "draws": 4}}}"#,
        )
        .unwrap();
        let runs = saved_runs(dir.path());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].engines, ["a", "b"]);
        assert_eq!(runs[0].games, 8);
    }

    /// Downloads the pinned release, plays a short match, stops it and continues it.
    #[test]
    #[ignore = "downloads fastchess; set FAST_TEST_ENGINE to a UCI engine"]
    fn download_play_stop_and_continue() {
        let engine = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        let data = tempfile::tempdir().unwrap();
        let program = install(data.path(), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!(installed(data.path()), Some(program.clone()));
        let dir = new_run_dir(data.path()).unwrap();
        let participant = |name: &str| Participant {
            name: name.into(),
            cmd: engine.clone(),
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant("a"), participant("b")]);
        setup.limit_value = "1+0.01".into();
        setup.rounds = "200".into();
        setup.concurrency = "2".into();
        setup.extra = String::new();
        let args = setup.args(&dir).unwrap();
        let collect = |process: &Process, stop_after: Option<u64>| {
            let mut finished = 0;
            let mut total = 0;
            let mut results = false;
            while let Ok(line) = process.lines.recv_blocking() {
                match parse_line(&line) {
                    Line::Started { total: all, .. } => total = all,
                    Line::Finished { .. } => {
                        finished += 1;
                        if Some(finished) == stop_after {
                            process.stop_handle().stop();
                        }
                    }
                    Line::Text(text) if text.starts_with("Results of") => results = true,
                    _ => {}
                }
            }
            (finished, total, results, process.wait())
        };
        let process = spawn(&program, &args, &dir, None).unwrap();
        let (finished, total, results, exit) = collect(&process, Some(4));
        assert!(
            finished >= 4 && total == 400 && results,
            "{finished} {total}"
        );
        assert!(exit.stopped && !exit.success);
        let config = dir.join("config.json");
        let saved = &saved_runs(data.path())[0];
        // With two games per round, only complete rounds are saved.
        assert!(
            saved.games.is_multiple_of(2) && saved.games <= finished,
            "{}",
            saved.games
        );
        let process = spawn(&program, &resume_args(&config), &dir, None).unwrap();
        let (finished, _, _, exit) = collect(&process, Some(2));
        assert!(finished >= 2 && exit.stopped);
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        assert!(count_games(&pgn) >= 6);
    }

    /// Verifies selfplay against any local fastchess build, including builds without events.
    #[test]
    #[ignore = "set FAST_TEST_FASTCHESS and FAST_TEST_ENGINE to local executables"]
    fn selfplay_runs_two_processes_of_the_same_engine_and_saves_legal_games() {
        let program = PathBuf::from(std::env::var("FAST_TEST_FASTCHESS").expect("fastchess"));
        let engine_path = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        let data = tempfile::tempdir().unwrap();
        let dir = new_run_dir(data.path()).unwrap();
        let participant = Participant {
            name: "Selfplay".into(),
            cmd: engine_path,
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant.clone(), participant]);
        setup.rounds = "1".into();
        setup.concurrency = "1".into();
        setup.limit = Limit::Nodes;
        setup.limit_value = "100".into();
        setup.maxmoves = "8".into();
        setup.sprt = false;
        setup.extra.clear();
        let process = spawn(&program, &setup.args(&dir).unwrap(), &dir, None).unwrap();
        let mut finished = 0;
        let mut output = Vec::new();
        while let Ok(line) = process.lines.recv_blocking() {
            if matches!(parse_line(&line), Line::Finished { .. }) {
                finished += 1;
            }
            output.push(line);
        }
        assert!(process.wait().success, "{output:?}");
        assert_eq!(finished, 2, "{output:?}");
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        let games = chess_core::parse_pgn(&pgn).unwrap();
        assert_eq!(games.len(), 2);
        for game in games {
            assert!(!game.mainline().is_empty());
            let names = [game.header("White").unwrap(), game.header("Black").unwrap()];
            assert!(names.contains(&"Selfplay"));
            assert!(names.contains(&"Selfplay (2)"));
            assert_ne!(game.header("Result"), Some("*"));
        }
    }

    /// Runs a fastchess build with `-events` and follows it as the Tournament tab does.
    #[test]
    #[ignore = "set FAST_TEST_FASTCHESS to a fastchess build with -events and FAST_TEST_ENGINE"]
    fn event_stream_drives_live_games() {
        use crate::tournament_live::{LiveGames, events_args, has_events};
        let program = PathBuf::from(std::env::var("FAST_TEST_FASTCHESS").expect("fastchess"));
        let engine = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        assert!(has_events(&program));
        let data = tempfile::tempdir().unwrap();
        let dir = new_run_dir(data.path()).unwrap();
        let participant = |name: &str| Participant {
            name: name.into(),
            cmd: engine.clone(),
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant("a"), participant("b")]);
        setup.limit_value = "2+0.02".into();
        setup.rounds = "50".into();
        setup.concurrency = "2".into();
        setup.extra = String::new();
        let mut args = setup.args(&dir).unwrap();
        args.extend(events_args());
        let live = Arc::new(Mutex::new(LiveGames::default()));
        let process = spawn(&program, &args, &dir, Some(live.clone())).unwrap();
        let mut finished = 0;
        let mut seen_moves = 0;
        while let Ok(line) = process.lines.recv_blocking() {
            assert!(
                !line.starts_with("{\"event\""),
                "events must not reach the log"
            );
            if let Line::Finished { .. } = parse_line(&line) {
                finished += 1;
                let games = live.lock().unwrap().snapshot();
                seen_moves = seen_moves.max(games.iter().map(|g| g.moves.len()).max().unwrap_or(0));
                assert!(
                    games
                        .iter()
                        .all(|game| game.number > 0 && game.white.is_some())
                );
                if finished == 3 {
                    process.stop_handle().stop();
                }
            }
        }
        assert!(process.wait().stopped);
        assert!(seen_moves > 10, "{seen_moves}");
        // Interrupted games are aborted, so no board is left behind.
        assert!(live.lock().unwrap().snapshot().is_empty());
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        let first = game_by_id(&pgn, 1).or_else(|| game_by_id(&pgn, 2)).unwrap();
        assert!(first.contains("[GameId "));
        // The event options are not saved for a later resume.
        let config = std::fs::read_to_string(dir.join("config.json")).unwrap();
        assert!(!config.contains("events"));
    }
}
