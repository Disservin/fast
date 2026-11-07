//! A running UCI process.
//!
//! A supervisor thread owns the child's stdin and a small state machine so
//! "stop, wait for bestmove, then start the next search" can never interleave.
//! Requesting a new search while one runs cancels it immediately.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use smol::channel::{self, Receiver, Sender};

use crate::{
    log::{EngineLog, LogKind},
    uci::{AnalysisUpdate, EngineInfo, UciLine, parse_line},
};

pub type EngineId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Limits {
    pub white_ms: Option<u64>,
    pub black_ms: Option<u64>,
    pub white_inc_ms: u64,
    pub black_inc_ms: u64,
    pub movetime_ms: Option<u64>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
}

impl Limits {
    fn go_command(self) -> String {
        let mut go = String::from("go");
        if let Some(depth) = self.depth {
            go.push_str(&format!(" depth {depth}"));
        }
        if let Some(movetime) = self.movetime_ms {
            go.push_str(&format!(" movetime {movetime}"));
        }
        if let Some(nodes) = self.nodes {
            go.push_str(&format!(" nodes {nodes}"));
        }
        if let (Some(white), Some(black)) = (self.white_ms, self.black_ms) {
            go.push_str(&format!(
                " wtime {white} btime {black} winc {} binc {}",
                self.white_inc_ms, self.black_inc_ms
            ));
        }
        go
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequest {
    /// `fen` is the starting FEN; `moves` are UCI moves from there.
    pub fen: String,
    pub moves: Vec<String>,
    /// Legal root moves to search, in UCI notation. Empty searches all moves.
    pub search_moves: Vec<String>,
    /// `None` means `go infinite`.
    pub limits: Option<Limits>,
    /// Caller-chosen token echoed in every event from this search.
    pub search_id: u64,
    /// Which game the position belongs to. A change sends `ucinewgame` first, so the
    /// engine does not carry assumptions from one game into another.
    pub game: u64,
}

impl SearchRequest {
    fn go_command(&self) -> String {
        let mut command = self
            .limits
            .map_or_else(|| "go infinite".to_owned(), Limits::go_command);
        if !self.search_moves.is_empty() {
            command.push_str(" searchmoves ");
            command.push_str(&self.search_moves.join(" "));
        }
        command
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    Ready(EngineInfo),
    Analysis {
        search_id: u64,
        update: AnalysisUpdate,
    },
    BestMove {
        search_id: u64,
        best: Option<String>,
    },
    /// The process exited or could not be used.
    Crashed(String),
}

enum Request {
    Search(SearchRequest),
    Stop,
    SetOption { name: String, value: Option<String> },
    NewGame,
    Debug(bool),
    Line(UciLine),
    Eof(String),
    Quit,
}

/// Cheap, cloneable control surface for one engine process.
#[derive(Clone)]
pub struct EngineHandle {
    commands: mpsc::Sender<Request>,
    events: Receiver<EngineEvent>,
    log: EngineLog,
}

impl EngineHandle {
    /// Spawns the process and performs the UCI handshake in the background,
    /// applying `options` once it succeeds. Spawn failures arrive as `Crashed`.
    pub fn spawn(path: PathBuf, args: Vec<String>, options: Vec<(String, String)>) -> Self {
        let (event_tx, event_rx) = channel::unbounded();
        let (command_tx, command_rx) = mpsc::channel();
        let supervisor_tx = command_tx.clone();
        let log = EngineLog::default();
        let supervisor_log = log.clone();
        let spawned = thread::Builder::new()
            .name("uci-supervisor".to_owned())
            .spawn(move || {
                supervise(
                    path,
                    args,
                    options,
                    command_rx,
                    supervisor_tx,
                    event_tx,
                    supervisor_log,
                )
            });
        if let Err(err) = spawned {
            log::error!("could not start engine thread: {err}");
        }
        Self {
            commands: command_tx,
            events: event_rx,
            log,
        }
    }

    /// Everything sent to and received from this process.
    pub fn log(&self) -> EngineLog {
        self.log.clone()
    }

    /// Stream of engine events. Clones share one queue.
    pub fn events(&self) -> Receiver<EngineEvent> {
        self.events.clone()
    }

    /// Cancels any running search, then starts this one.
    pub fn search(&self, request: SearchRequest) {
        let _ = self.commands.send(Request::Search(request));
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Request::Stop);
    }

    pub fn set_option(&self, name: impl Into<String>, value: Option<String>) {
        let _ = self.commands.send(Request::SetOption {
            name: name.into(),
            value,
        });
    }

    /// Turns the engine's UCI debug mode on or off (`debug on` / `debug off`).
    pub fn set_debug(&self, on: bool) {
        let _ = self.commands.send(Request::Debug(on));
    }

    pub fn new_game(&self) {
        let _ = self.commands.send(Request::NewGame);
    }

    pub fn quit(&self) {
        let _ = self.commands.send(Request::Quit);
    }
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    log: EngineLog,
}

impl Process {
    fn send(&mut self, line: &str) -> std::io::Result<()> {
        log::trace!("uci> {line}");
        self.log.push(LogKind::Sent, line);
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        // Give it a moment to exit cleanly before killing.
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_process(
    path: &PathBuf,
    args: &[String],
    supervisor: mpsc::Sender<Request>,
    engine_log: EngineLog,
) -> std::io::Result<Process> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        command.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no stdout"))?;
    if let Some(stderr) = child.stderr.take() {
        let stderr_log = engine_log.clone();
        thread::Builder::new()
            .name("uci-stderr".to_owned())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    stderr_log.push(LogKind::Stderr, line);
                }
            })?;
    }
    let reader_log = engine_log.clone();
    thread::Builder::new()
        .name("uci-reader".to_owned())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                log::trace!("uci< {line}");
                reader_log.push(LogKind::Received, line.as_str());
                if supervisor.send(Request::Line(parse_line(&line))).is_err() {
                    return;
                }
            }
            let _ = supervisor.send(Request::Eof("engine process exited".to_owned()));
        })?;
    Ok(Process {
        child,
        stdin,
        log: engine_log,
    })
}

/// The standard initial position, sent as `position startpos` rather than as a FEN.
pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// The `position` command for a start FEN and moves, using `startpos` for the
/// standard position as the protocol intends.
pub fn position_command(fen: &str, moves: &[String]) -> String {
    let base = if fen.trim() == START_FEN {
        "position startpos".to_owned()
    } else {
        format!("position fen {}", fen.trim())
    };
    if moves.is_empty() {
        base
    } else {
        format!("{base} moves {}", moves.join(" "))
    }
}

/// Search bookkeeping of the supervisor.
#[derive(Default)]
struct Session {
    /// Search in flight: told `go` and not yet answered `bestmove`.
    running: Option<u64>,
    stop_sent: bool,
    /// Latest search waiting for the current one to finish; older ones are dropped.
    pending: Option<SearchRequest>,
    /// The search in flight, so it can resume after an option change.
    current: Option<SearchRequest>,
    /// Options wait for the engine to go idle, as UCI requires.
    pending_options: Vec<(String, Option<String>)>,
    /// Options or `ucinewgame` were sent since the last `readyok`: the next search
    /// waits for `isready` to be answered, so the engine can finish initialising.
    needs_sync: bool,
    /// `isready` sent, `readyok` not yet received.
    awaiting_ready: bool,
    /// The search to start once `readyok` arrives.
    after_ready: Option<SearchRequest>,
    /// The game the last search belonged to, to send `ucinewgame` on a change.
    last_game: Option<u64>,
}

impl Session {
    /// Starts a search on an idle engine: a new game first gets `ucinewgame`, and any
    /// unsynchronised change gets `isready` before `go`.
    fn launch(&mut self, process: &mut Process, request: SearchRequest) -> std::io::Result<()> {
        if self.last_game != Some(request.game) {
            process.send("ucinewgame")?;
            self.last_game = Some(request.game);
            self.needs_sync = true;
        }
        if self.awaiting_ready {
            self.after_ready = Some(request);
            return Ok(());
        }
        if self.needs_sync {
            process.send("isready")?;
            self.needs_sync = false;
            self.awaiting_ready = true;
            self.after_ready = Some(request);
            return Ok(());
        }
        process.send(&position_command(&request.fen, &request.moves))?;
        process.send(&request.go_command())?;
        self.running = Some(request.search_id);
        self.current = Some(request);
        Ok(())
    }

    /// Asks for a search: started now if idle, otherwise after the running one stops.
    fn request(&mut self, process: &mut Process, request: SearchRequest) -> std::io::Result<()> {
        if self.running.is_some() {
            self.pending = Some(request);
            if self.stop_sent {
                Ok(())
            } else {
                self.stop_sent = true;
                process.send("stop")
            }
        } else {
            self.launch(process, request)
        }
    }

    fn set_option(
        &mut self,
        process: &mut Process,
        name: String,
        value: Option<String>,
    ) -> std::io::Result<()> {
        if self.running.is_some() {
            // Stop, apply when idle, then carry on with the same search.
            self.pending_options.push((name, value));
            if self.pending.is_none() {
                self.pending = self.current.clone();
            }
            if self.stop_sent {
                return Ok(());
            }
            self.stop_sent = true;
            return process.send("stop");
        }
        self.needs_sync = true;
        process.send(&set_option_line(&name, value.as_deref()))
    }

    fn ready_ok(&mut self, process: &mut Process) -> std::io::Result<()> {
        self.awaiting_ready = false;
        let Some(request) = self.after_ready.take() else {
            return Ok(());
        };
        // Something changed while waiting: synchronise again first.
        self.launch(process, request)
    }

    fn best_move(&mut self, process: &mut Process) -> std::io::Result<Option<u64>> {
        let finished = self.running.take().filter(|_| !self.stop_sent);
        self.stop_sent = false;
        let mut result = Ok(());
        for (name, value) in std::mem::take(&mut self.pending_options) {
            result = result.and(process.send(&set_option_line(&name, value.as_deref())));
            self.needs_sync = true;
        }
        self.current = None;
        if let Some(next) = self.pending.take() {
            result = result.and(self.launch(process, next));
        }
        result.map(|()| finished)
    }

    fn stop(&mut self, process: &mut Process) -> std::io::Result<()> {
        self.pending = None;
        self.after_ready = None;
        if self.running.is_some() && !self.stop_sent {
            self.stop_sent = true;
            process.send("stop")
        } else {
            Ok(())
        }
    }
}

fn supervise(
    path: PathBuf,
    args: Vec<String>,
    options: Vec<(String, String)>,
    commands: mpsc::Receiver<Request>,
    self_tx: mpsc::Sender<Request>,
    events: Sender<EngineEvent>,
    engine_log: EngineLog,
) {
    let emit = |event: EngineEvent| {
        if let EngineEvent::Crashed(reason) = &event {
            engine_log.push(LogKind::System, reason.as_str());
        }
        let _ = events.send_blocking(event);
    };
    engine_log.push(LogKind::System, format!("starting {}", path.display()));
    let mut process = match start_process(&path, &args, self_tx, engine_log.clone()) {
        Ok(process) => process,
        Err(err) => {
            emit(EngineEvent::Crashed(format!(
                "could not start {}: {err}",
                path.display()
            )));
            return;
        }
    };
    if let Err(err) = process.send("uci") {
        emit(EngineEvent::Crashed(format!(
            "could not talk to engine: {err}"
        )));
        return;
    }

    let mut info = EngineInfo::default();
    let mut handshake_done = false;
    let mut session = Session {
        // The protocol requires `isready` once before the first search.
        needs_sync: true,
        ..Default::default()
    };
    // Requests arriving before the handshake finishes, replayed in order.
    let mut deferred: VecDeque<Request> = VecDeque::new();

    while let Ok(command) = commands.recv() {
        let command = match command {
            Request::Search(_)
            | Request::SetOption { .. }
            | Request::NewGame
            | Request::Debug(_)
                if !handshake_done =>
            {
                deferred.push_back(command);
                continue;
            }
            other => other,
        };
        let mut result = match command {
            Request::Line(line) => match line {
                UciLine::Id { key, value } => {
                    match key.as_str() {
                        "name" => info.name = value,
                        "author" => info.author = value,
                        _ => {}
                    }
                    Ok(())
                }
                UciLine::Option(option) => {
                    info.options.push(option);
                    Ok(())
                }
                UciLine::UciOk => {
                    let mut result = Ok(());
                    for (name, value) in &options {
                        result = result.and(process.send(&set_option_line(name, Some(value))));
                    }
                    handshake_done = true;
                    emit(EngineEvent::Ready(info.clone()));
                    result
                }
                UciLine::Info(update) => {
                    // Output after `stop` belongs to a cancelled search: drop it so a
                    // stale line can never overwrite the new position's analysis.
                    if let Some(search_id) = session.running.filter(|_| !session.stop_sent) {
                        emit(EngineEvent::Analysis { search_id, update });
                    }
                    Ok(())
                }
                UciLine::BestMove { best, .. } => match session.best_move(&mut process) {
                    Ok(Some(search_id)) => {
                        emit(EngineEvent::BestMove { search_id, best });
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(err) => Err(err),
                },
                UciLine::ReadyOk => session.ready_ok(&mut process),
                UciLine::Other => Ok(()),
            },
            Request::Search(request) => session.request(&mut process, request),
            Request::Stop => session.stop(&mut process),
            Request::SetOption { name, value } => session.set_option(&mut process, name, value),
            Request::NewGame => {
                session.needs_sync = true;
                process.send("ucinewgame")
            }
            Request::Debug(on) => process.send(if on { "debug on" } else { "debug off" }),
            Request::Eof(reason) => {
                emit(EngineEvent::Crashed(reason));
                return;
            }
            Request::Quit => return,
        };
        if handshake_done {
            while result.is_ok()
                && let Some(command) = deferred.pop_front()
            {
                result = match command {
                    Request::Search(request) => session.request(&mut process, request),
                    Request::SetOption { name, value } => {
                        session.set_option(&mut process, name, value)
                    }
                    Request::NewGame => {
                        session.needs_sync = true;
                        process.send("ucinewgame")
                    }
                    Request::Debug(on) => process.send(if on { "debug on" } else { "debug off" }),
                    _ => Ok(()),
                };
            }
        }
        if let Err(err) = result {
            emit(EngineEvent::Crashed(format!("engine write failed: {err}")));
            return;
        }
    }
}

fn set_option_line(name: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("setoption name {name} value {value}"),
        None => format!("setoption name {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_position_is_sent_as_startpos() {
        assert_eq!(position_command(START_FEN, &[]), "position startpos");
        assert_eq!(
            position_command(START_FEN, &["e2e4".to_owned(), "e7e5".to_owned()]),
            "position startpos moves e2e4 e7e5"
        );
        let fen = "8/8/8/8/8/8/k7/K7 w - - 0 1";
        assert_eq!(position_command(fen, &[]), format!("position fen {fen}"));
    }
}
