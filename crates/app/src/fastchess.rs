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

pub const VERSION: &str = "1.8.2-alpha";

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
                "1003f920bebe841acdab5e6d7871e93171a4586857b3ec91c21ef5777f26ff96",
            ),
            asset(
                "linux",
                "aarch64",
                "linux-arm64",
                "fe079f8784394488baa199d4196768e46ccab0b31741d67c9cd70db0a237a494",
            ),
            asset(
                "macos",
                "x86_64",
                "mac-x86-64",
                "c97c62cdfc52449b901238c7326de5e416434f0f98c979b6e16129c5d41affcc",
            ),
            asset(
                "macos",
                "aarch64",
                "mac-arm64",
                "a736371b1df3d7c67d4288dee66f6bb4e7e8193a945639c8a1d0aabfa612b551",
            ),
            asset(
                "windows",
                "x86_64",
                "windows-x86-64",
                "007ce550dc809510b34622a1aeb43fd443d40284045bef5c1bc39c8941114143",
            ),
            asset(
                "windows",
                "aarch64",
                "windows-arm64",
                "0349179a2d61343f2516583f8c7b056d254b8bbbda75c71ff9ed740c49676eeb",
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
    pub openings: String,
    pub random_order: bool,
    pub plies: String,
    pub sprt: bool,
    pub elo0: String,
    pub elo1: String,
    pub alpha: String,
    pub beta: String,
    pub draw_adjudication: bool,
    pub resign_adjudication: bool,
    pub maxmoves: String,
    pub recover: bool,
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
                if !SKIPPED_OPTIONS.contains(&option.as_str()) {
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
            ]);
        }
        if self.draw_adjudication {
            push(&[
                "-draw".into(),
                "movenumber=32".into(),
                "movecount=6".into(),
                "score=10".into(),
            ]);
        }
        if self.resign_adjudication {
            push(&[
                "-resign".into(),
                "movecount=3".into(),
                "score=600".into(),
                "twosided=true".into(),
            ]);
        }
        if !self.maxmoves.trim().is_empty() {
            push(&[
                "-maxmoves".into(),
                number::<u32>(&self.maxmoves, "Maximum moves")?.to_string(),
            ]);
        }
        if self.recover {
            push(&["-recover".into()]);
        }
        // Results after every game keep the table live.
        push(&["-ratinginterval".into(), "1".into()]);
        push(&split_args(&self.extra)?);
        args.extend(output_args(dir));
        Ok(args)
    }
}

/// Games and the resumable config go into the tournament's folder.
fn output_args(dir: &Path) -> Vec<String> {
    vec![
        "-pgnout".into(),
        format!("file={}", dir.join("games.pgn").display()),
        "-config".into(),
        format!("outname={}", dir.join("config.json").display()),
    ]
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

/// Headline numbers from a head-to-head results block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub heading: Option<String>,
    pub elo: Option<String>,
    pub los: Option<String>,
    pub games: Option<String>,
    pub ptnml: Option<String>,
    pub llr: Option<String>,
}

pub fn summarize(block: &[String]) -> Summary {
    let mut summary = Summary::default();
    for line in block {
        let line = line.trim();
        let field = |prefix: &str| line.strip_prefix(prefix).map(|rest| rest.trim().to_owned());
        if line.starts_with("Results of ") {
            summary.heading = Some(line.trim_end_matches(':').to_owned());
        } else if let Some(value) = field("Elo:") {
            summary.elo = Some(value);
        } else if let Some(value) = field("LOS:") {
            summary.los = Some(value);
        } else if let Some(value) = field("Games:") {
            summary.games = Some(value);
        } else if let Some(value) = field("Ptnml(0-2):") {
            summary.ptnml = Some(value);
        } else if let Some(value) = field("LLR:") {
            summary.llr = Some(value);
        }
    }
    summary
}

/// A running fastchess process. Output lines arrive on `lines`; the channel closes when
/// the process has exited and its output is read.
pub struct Process {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
    pub lines: smol::channel::Receiver<String>,
}

#[derive(Clone)]
pub struct StopHandle {
    child: Arc<Mutex<Child>>,
    stopping: Arc<AtomicBool>,
}

/// Starts fastchess. Engine-log lines go to `live` instead of `lines` when it is given.
pub fn spawn(
    program: &Path,
    args: &[String],
    cwd: &Path,
    live: Option<Arc<Mutex<crate::tournament_live::LiveGames>>>,
) -> Result<Process> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // fastchess still gets a (hidden) console, which the engines it starts share.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Starting {}", program.display()))?;
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
                    && line.starts_with("[Engine]")
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
        lines,
    })
}

impl Process {
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle {
            child: self.child.clone(),
            stopping: self.stopping.clone(),
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
    /// call, or any call on Windows (which has no signal to send), kills it outright.
    pub fn stop(&self) {
        let again = self.stopping.swap(true, Ordering::Relaxed);
        let Ok(mut child) = self.child.lock() else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        if !again {
            let Ok(pid) = i32::try_from(child.id()) else {
                return;
            };
            // SAFETY: plain signal to a child we own and have not reaped.
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
            return;
        }
        let _ = again;
        let _ = child.kill();
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
            openings: String::new(),
            random_order: true,
            plies: String::new(),
            sprt: true,
            elo0: "0".into(),
            elo1: "2".into(),
            alpha: "0.05".into(),
            beta: "0.05".into(),
            draw_adjudication: false,
            resign_adjudication: false,
            maxmoves: String::new(),
            recover: true,
            extra: "-srand 42".into(),
        }
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
        let summary = summarize(&block);
        assert_eq!(
            summary.heading.as_deref(),
            Some("Results of a vs b (1+0.01, 1t, 8MB)")
        );
        assert_eq!(
            summary.elo.as_deref(),
            Some("-58.45 +/- 98.05, nElo: -173.72 +/- 278.00")
        );
        assert!(summary.llr.unwrap().starts_with("-0.03"));
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
        assert!(saved.games >= 4, "{}", saved.games);
        let process = spawn(&program, &resume_args(&config), &dir, None).unwrap();
        let (finished, _, _, exit) = collect(&process, Some(2));
        assert!(finished >= 2 && exit.stopped);
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        assert!(count_games(&pgn) >= 6);
    }
}
