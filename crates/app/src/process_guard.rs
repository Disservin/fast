//! Keeps fastchess from outliving Fast, however Fast ends.
//!
//! A normal quit and the Stop button stop fastchess themselves, but a crash or a kill gives
//! Fast no chance to. So:
//!
//! - **Unix:** fastchess runs under a guard, Fast's own binary started with [`FLAG`]. Fast
//!   holds the write end of the guard's stdin; when Fast dies, however it dies, the guard
//!   reads end-of-file and sends fastchess SIGINT, as Ctrl+C would, so fastchess saves its
//!   config and stops its engines. The guard forwards SIGINT, SIGTERM and SIGHUP the same
//!   way, kills fastchess on SIGUSR2, and exits with fastchess's status. On Linux fastchess
//!   also gets SIGINT if the guard itself is killed.
//! - **Windows:** fastchess goes into a job object that kills every process in it, engines
//!   included, once its last handle closes, which happens when Fast exits. Windows has no
//!   Ctrl+C to send to a process without a console, so this is a hard stop, like Stop.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs Fast as a guard: `fast --fastchess-guard <program> [args…]`.
pub const FLAG: &str = "--fastchess-guard";

/// Fast's executable for starting a guard. Unit tests run inside the test harness, which has
/// no guard mode, so they start fastchess directly.
fn guard_executable() -> Option<PathBuf> {
    if cfg!(test) || !cfg!(unix) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    if exe.is_file() {
        return Some(exe);
    }
    // The binary was replaced while running (a rebuild); Linux still offers the running one.
    cfg!(target_os = "linux").then(|| PathBuf::from("/proc/self/exe"))
}

/// A command that starts `program`, under a guard where there is one. Returns whether it is
/// guarded; a guarded command needs its stdin piped and kept open while fastchess runs.
pub fn command(program: &Path) -> (Command, bool) {
    match guard_executable() {
        Some(guard) => {
            let mut command = Command::new(guard);
            command.arg(FLAG).arg(program);
            (command, true)
        }
        None => (Command::new(program), false),
    }
}

/// The guard's entry point, when Fast was started with [`FLAG`]. Returns the exit code.
pub fn run_if_requested() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != FLAG {
        return None;
    }
    let args: Vec<_> = args.collect();
    #[cfg(unix)]
    {
        Some(unix::run(&args))
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        eprintln!("{FLAG} is only used on Unix");
        Some(2)
    }
}

#[cfg(unix)]
mod unix {
    use std::{
        ffi::OsString,
        io::Read,
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    static STOP: AtomicBool = AtomicBool::new(false);
    static KILL: AtomicBool = AtomicBool::new(false);
    static PARENT_GONE: AtomicBool = AtomicBool::new(false);

    /// How long fastchess gets to stop after Fast is gone before it is killed.
    const GRACE: Duration = Duration::from_secs(60);

    extern "C" fn on_stop(_: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }

    extern "C" fn on_kill(_: libc::c_int) {
        KILL.store(true, Ordering::Relaxed);
    }

    pub fn run(args: &[OsString]) -> i32 {
        let Some((program, args)) = args.split_first() else {
            eprintln!("{}: no program given", super::FLAG);
            return 2;
        };
        // SAFETY: the handlers only store to atomics, which is async-signal-safe.
        unsafe {
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::signal(
                    signal,
                    on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t,
                );
            }
            libc::signal(
                libc::SIGUSR2,
                on_kill as extern "C" fn(libc::c_int) as libc::sighandler_t,
            );
        }
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let guard = std::process::id();
            // SAFETY: prctl and getppid are async-signal-safe.
            unsafe {
                command.pre_exec(move || {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT);
                    // The guard may already be gone before the request took effect.
                    if libc::getppid() as u32 != guard {
                        libc::raise(libc::SIGINT);
                    }
                    Ok(())
                });
            }
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("Could not start {}: {err}", program.to_string_lossy());
                return 127;
            }
        };
        // Fast never writes here; end-of-file means it has exited.
        std::thread::spawn(|| {
            let mut stdin = std::io::stdin();
            let mut buffer = [0u8; 64];
            while matches!(stdin.read(&mut buffer), Ok(read) if read > 0) {}
            PARENT_GONE.store(true, Ordering::Relaxed);
        });
        let pid = child.id() as libc::pid_t;
        let mut deadline: Option<Instant> = None;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return status
                        .code()
                        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
                }
                Ok(None) => {}
                Err(_) => return 1,
            }
            // The child is not reaped yet, so its pid is still ours to signal.
            let parent_gone = deadline.is_none() && PARENT_GONE.load(Ordering::Relaxed);
            if STOP.swap(false, Ordering::Relaxed) || parent_gone {
                // SAFETY: plain signal to our own unreaped child.
                unsafe {
                    libc::kill(pid, libc::SIGINT);
                }
            }
            if parent_gone {
                deadline = Some(Instant::now() + GRACE);
            }
            if KILL.swap(false, Ordering::Relaxed)
                || deadline.is_some_and(|deadline| Instant::now() >= deadline)
            {
                let _ = child.kill();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A Windows job object that kills its processes once its last handle closes.
#[cfg(windows)]
pub struct Job(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a job handle may be used and closed from any thread.
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
unsafe impl Sync for Job {}

#[cfg(windows)]
impl Job {
    /// Puts `child` (and the processes it starts from now on) into a new job.
    pub fn attach(child: &std::process::Child) -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: plain Win32 calls on handles we own; the job is closed by `Drop`.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(job.0, child.as_raw_handle() as _) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(job)
        }
    }

    /// Ends every process in the job: fastchess and its engines.
    pub fn terminate(&self) {
        // SAFETY: the handle is valid until `Drop`.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing our own handle; with it the job's processes end, if any are left.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
