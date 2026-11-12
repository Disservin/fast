//! Running fastchess: starting the process, reading its output and stopping it.

use super::*;

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
