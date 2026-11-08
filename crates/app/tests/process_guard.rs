//! The guard that stops fastchess when Fast dies (`fast --fastchess-guard <program> …`),
//! run with a shell script in place of fastchess.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

/// A stand-in for fastchess: says "ready", then on SIGINT records it and exits with 3.
fn script(marker: &Path) -> String {
    format!(
        "trap 'echo stopped > \"{}\"; echo stopped; exit 3' INT; echo ready; \
         while :; do sleep 0.05; done",
        marker.display()
    )
}

fn guard(script: &str) -> (Child, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fast"))
        .args(["--fastchess-guard", "sh", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "ready");
    (child, stdout)
}

fn signal(child: &Child, signal: libc::c_int) {
    // SAFETY: plain signal to our own unreaped child.
    unsafe {
        libc::kill(child.id() as libc::pid_t, signal);
    }
}

fn wait(child: &mut Child) -> i32 {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().unwrap();
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "guard did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until the stand-in child has written its "stopped" marker.
#[cfg(target_os = "linux")]
fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "child was not stopped"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn next_line(stdout: &mut BufReader<ChildStdout>) -> String {
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    line.trim().to_owned()
}

#[test]
fn closing_stdin_like_a_dying_fast_stops_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("stopped");
    let (mut child, mut stdout) = guard(&script(&marker));
    drop(child.stdin.take());
    assert_eq!(next_line(&mut stdout), "stopped");
    assert_eq!(wait(&mut child), 3, "the child's exit code passes through");
}

#[test]
fn stop_signals_reach_the_child() {
    for sent in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("stopped");
        let (mut child, mut stdout) = guard(&script(&marker));
        signal(&child, sent);
        assert_eq!(next_line(&mut stdout), "stopped", "signal {sent}");
        assert_eq!(wait(&mut child), 3);
    }
}

#[test]
fn sigusr2_kills_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, _stdout) = guard(&script(&dir.path().join("stopped")));
    signal(&child, libc::SIGUSR2);
    assert_eq!(wait(&mut child), 128 + libc::SIGKILL);
    assert!(!dir.path().join("stopped").exists());
}

#[test]
fn exit_codes_pass_through() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fast"))
        .args(["--fastchess-guard", "sh", "-c", "exit 5"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child), 5);
    let mut child = Command::new(env!("CARGO_BIN_EXE_fast"))
        .args(["--fastchess-guard", "/no/such/program"])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child), 127);
}

#[cfg(target_os = "linux")]
#[test]
fn killing_the_guard_itself_still_stops_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("stopped");
    let (mut child, _stdout) = guard(&script(&marker));
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for(&marker);
}
