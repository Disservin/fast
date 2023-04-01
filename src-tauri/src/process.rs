use std::io::{BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

pub struct InnerProcessState {
    pub process: Option<std::process::Child>,
}

impl InnerProcessState {
    pub fn start_process(&mut self, command: &str) {
        let process = Command::new(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to start process");
        self.process = Some(process);
    }

    pub fn end_process(&mut self) {
        if let Some(process) = &mut self.process {
            process.kill().expect("failed to kill process");
            self.process = None;
        }
    }

    pub fn send_string(&mut self, string: &str) {
        if let Some(process) = &mut self.process {
            process
                .stdin
                .as_mut()
                .expect("failed to open stdin")
                .write_all(string.as_bytes())
                .expect("failed to write to stdin");
        }
    }
}

pub struct ProcessState(pub Mutex<InnerProcessState>);

#[tauri::command]
pub fn start_process<'a>(state: tauri::State<'a, ProcessState>, command: String) {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.start_process(&command);
}

#[tauri::command]
pub fn end_process<'a>(state: tauri::State<'a, ProcessState>) {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.end_process();
}

#[tauri::command]
pub fn send_string<'a>(state: tauri::State<'a, ProcessState>, input: String) {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.send_string(&input);
}
