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

use crate::uci::{AnalysisUpdate, EngineInfo, UciLine, parse_line};

pub type EngineId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Limits {
    pub white_ms: Option<u64>,
    pub black_ms: Option<u64>,
    pub white_inc_ms: u64,
    pub black_inc_ms: u64,
    pub movetime_ms: Option<u64>,
    pub depth: Option<u32>,
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
    /// `None` means `go infinite`.
    pub limits: Option<Limits>,
    /// Caller-chosen token echoed in every event from this search.
    pub search_id: u64,
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
    Line(UciLine),
    Eof(String),
    Quit,
}

/// Cheap, cloneable control surface for one engine process.
#[derive(Clone)]
pub struct EngineHandle {
    commands: mpsc::Sender<Request>,
    events: Receiver<EngineEvent>,
}

impl EngineHandle {
    /// Spawns the process and performs the UCI handshake in the background,
    /// applying `options` once it succeeds. Spawn failures arrive as `Crashed`.
    pub fn spawn(path: PathBuf, args: Vec<String>, options: Vec<(String, String)>) -> Self {
        let (event_tx, event_rx) = channel::unbounded();
        let (command_tx, command_rx) = mpsc::channel();
        let supervisor_tx = command_tx.clone();
        let spawned = thread::Builder::new()
            .name("uci-supervisor".to_owned())
            .spawn(move || supervise(path, args, options, command_rx, supervisor_tx, event_tx));
        if let Err(err) = spawned {
            log::error!("could not start engine thread: {err}");
        }
        Self {
            commands: command_tx,
            events: event_rx,
        }
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
}

impl Process {
    fn send(&mut self, line: &str) -> std::io::Result<()> {
        log::trace!("uci> {line}");
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
) -> std::io::Result<Process> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
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
    thread::Builder::new()
        .name("uci-reader".to_owned())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                log::trace!("uci< {line}");
                if supervisor.send(Request::Line(parse_line(&line))).is_err() {
                    return;
                }
            }
            let _ = supervisor.send(Request::Eof("engine process exited".to_owned()));
        })?;
    Ok(Process { child, stdin })
}

fn supervise(
    path: PathBuf,
    args: Vec<String>,
    options: Vec<(String, String)>,
    commands: mpsc::Receiver<Request>,
    self_tx: mpsc::Sender<Request>,
    events: Sender<EngineEvent>,
) {
    let emit = |event| {
        let _ = events.send_blocking(event);
    };
    let mut process = match start_process(&path, &args, self_tx) {
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
    // Search in flight (engine has been told `go` and not yet answered `bestmove`).
    let mut running: Option<u64> = None;
    let mut stop_sent = false;
    // Latest search awaiting the current one to finish; older ones are dropped.
    let mut pending: Option<SearchRequest> = None;
    // Writes arriving before the handshake finishes, replayed in order.
    let mut deferred: VecDeque<Request> = VecDeque::new();

    fn begin(process: &mut Process, request: &SearchRequest) -> std::io::Result<()> {
        let moves = if request.moves.is_empty() {
            String::new()
        } else {
            format!(" moves {}", request.moves.join(" "))
        };
        process.send(&format!("position fen {}{moves}", request.fen))?;
        match request.limits {
            Some(limits) => process.send(&limits.go_command()),
            None => process.send("go infinite"),
        }
    }

    while let Ok(command) = commands.recv() {
        let result = match command {
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
                    if let Some(search_id) = running.filter(|_| !stop_sent) {
                        emit(EngineEvent::Analysis { search_id, update });
                    }
                    Ok(())
                }
                UciLine::BestMove { best, .. } => {
                    if let Some(search_id) = running.take()
                        && !stop_sent
                    {
                        emit(EngineEvent::BestMove { search_id, best });
                    }
                    stop_sent = false;
                    if let Some(next) = pending.take() {
                        let result = begin(&mut process, &next);
                        running = Some(next.search_id);
                        result
                    } else {
                        Ok(())
                    }
                }
                UciLine::ReadyOk | UciLine::Other => Ok(()),
            },
            Request::Search(request) if !handshake_done => {
                deferred.push_back(Request::Search(request));
                Ok(())
            }
            Request::SetOption { name, value } if !handshake_done => {
                deferred.push_back(Request::SetOption { name, value });
                Ok(())
            }
            Request::Search(request) => {
                if running.is_some() {
                    pending = Some(request);
                    if stop_sent {
                        Ok(())
                    } else {
                        stop_sent = true;
                        process.send("stop")
                    }
                } else {
                    let result = begin(&mut process, &request);
                    running = Some(request.search_id);
                    result
                }
            }
            Request::Stop => {
                pending = None;
                if running.is_some() && !stop_sent {
                    stop_sent = true;
                    process.send("stop")
                } else {
                    Ok(())
                }
            }
            Request::SetOption { name, value } => {
                process.send(&set_option_line(&name, value.as_deref()))
            }
            Request::NewGame if !handshake_done => {
                deferred.push_back(Request::NewGame);
                Ok(())
            }
            Request::NewGame => process.send("ucinewgame"),
            Request::Eof(reason) => {
                emit(EngineEvent::Crashed(reason));
                return;
            }
            Request::Quit => return,
        };
        if let Err(err) = result {
            emit(EngineEvent::Crashed(format!("engine write failed: {err}")));
            return;
        }
        if handshake_done {
            while let Some(command) = deferred.pop_front() {
                match command {
                    Request::Search(request) => {
                        if running.is_some() {
                            pending = Some(request);
                            if !stop_sent {
                                stop_sent = true;
                                let _ = process.send("stop");
                            }
                        } else {
                            let _ = begin(&mut process, &request);
                            running = Some(request.search_id);
                        }
                    }
                    Request::SetOption { name, value } => {
                        let _ = process.send(&set_option_line(&name, value.as_deref()));
                    }
                    Request::NewGame => {
                        let _ = process.send("ucinewgame");
                    }
                    _ => {}
                }
            }
        }
    }
}

fn set_option_line(name: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("setoption name {name} value {value}"),
        None => format!("setoption name {name}"),
    }
}
