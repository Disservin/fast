use std::{
    io::{self, BufRead, BufReader, Write},
    process::{Child, ChildStdout, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const READLINE_TIMEOUT: Duration = Duration::from_millis(100);

pub struct Engine {
    pub process: Option<Child>,
    pub reader: Option<BufReader<ChildStdout>>,
    pub clock: Option<Instant>,
}

impl Engine {
    pub fn new(&mut self, command: &String) {
        let mut process = Command::new(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn engine process");

        self.reader = Some(BufReader::new(
            process.stdout.take().expect("Stdout is piped"),
        ));

        self.process = Some(process);

        self.uci().expect("Failed to send uci command");
    }

    fn write(&mut self, cmd: &str) -> io::Result<()> {
        if self.process.is_none() {
            return Ok(());
        }

        let stdin = self
            .process
            .as_mut()
            .unwrap()
            .stdin
            .as_mut()
            .expect("Stdin is piped");
        stdin.write_all(cmd.as_bytes())?;
        stdin.write_all(b"\n")?;

        Ok(())
    }

    fn read_line(&mut self) -> Result<String, String> {
        if self.reader.is_none() {
            return Err("ReadlineTimeout".to_string());
        }

        let mut s = String::new();

        let timer = Instant::now();
        while timer.elapsed() < READLINE_TIMEOUT {
            if self.reader.as_mut().unwrap().read_line(&mut s).is_ok() {
                return Ok(s);
            }
        }
        Err("ReadlineTimeout".to_string())
    }

    pub fn expect(&mut self, expected: &str) -> Result<String, String> {
        while let Ok(line) = self.read_line() {
            if line.starts_with(expected) {
                return Ok(line);
            }
        }

        Err("Didnt find expected string".to_string())
    }

    pub fn isready(&mut self) -> Result<(), String> {
        let _ = self.write("isready");
        let line = self.read_line();
        if line.unwrap() == "readyok" {
            Ok(())
        } else {
            Err("NoReadyok".to_string())
        }
    }

    pub fn uci(&mut self) -> Result<(), String> {
        let _ = self.write("uci");
        let _ = self.expect("uciok");

        Ok(())
    }

    pub fn position_fen(&mut self, start: &str, moves: &str) -> io::Result<()> {
        self.write(&format!("position fen {} moves {}", start, moves))
    }

    pub fn position_startpos(&mut self, moves: &str) -> io::Result<()> {
        self.write(&format!("position startpos moves {}", (moves)))
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> io::Result<()> {
        self.write(&format!("setoption name {} value {}", name, value))
    }

    pub fn get_options(&mut self) -> Vec<String> {
        let expected = "uciok";

        let mut lines: Vec<String> = Vec::new();

        self.write("uci").expect("No uci");
        while let Ok(line) = self.read_line() {
            lines.push(line.to_owned());
            if line.starts_with(expected) {
                return lines;
            }
        }
        return lines;
    }

    pub fn new_game(&mut self) -> io::Result<()> {
        self.write("ucinewgame")
    }

    pub fn stop(&mut self) -> io::Result<()> {
        self.write("stop")
    }

    pub fn get_bestmove(&mut self) -> Result<String, String> {
        let bestmove = self.expect("bestmove").map_err(|_| "Error::NoBestmove")?;
        let move_string = bestmove
            .split_whitespace()
            .nth(1)
            .ok_or("Error::NoBestmove")?;

        Ok(move_string.to_string())
    }

    pub fn go(&mut self) -> io::Result<()> {
        self.write(&format!("go infinite"))
    }

    pub fn go_nodes(&mut self, nodes: usize) -> io::Result<()> {
        self.write(&format!("go nodes {}", nodes))
    }

    pub fn quit(&mut self) -> io::Result<()> {
        self.write("quit")
    }
}

pub struct MyState(pub Mutex<Engine>);

#[tauri::command]
pub async fn new(state: tauri::State<'_, MyState>, command: String) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.new(&command);
    Ok(())
}

#[tauri::command]
pub async fn write(state: tauri::State<'_, MyState>, command: String) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.write(&command).unwrap(); // Ignore the Result
    Ok(())
}

#[tauri::command]
pub async fn read_line(state: tauri::State<'_, MyState>) -> Result<String, String> {
    let mut state_guard = state.0.lock().unwrap();
    Ok(state_guard.read_line().unwrap())
}

#[tauri::command]
pub async fn isready(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.isready().unwrap();
    Ok(())
}

#[tauri::command]
pub async fn uci(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.uci().unwrap();
    Ok(())
}

#[tauri::command]
pub async fn position_fen(
    state: tauri::State<'_, MyState>,
    start: String,
    moves: String,
) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.position_fen(&start, &moves).unwrap();
    Ok(())
}

#[tauri::command]
pub async fn position_startpos(
    state: tauri::State<'_, MyState>,
    moves: String,
) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.position_startpos(&moves).unwrap();
    Ok(())
}

#[tauri::command]
pub async fn set_option(
    state: tauri::State<'_, MyState>,
    name: String,
    value: String,
) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.set_option(&name, &value).unwrap();
    Ok(())
}

#[tauri::command]
pub async fn get_options(state: tauri::State<'_, MyState>) -> Result<Vec<String>, String> {
    let mut state_guard = state.0.lock().unwrap();
    Ok(state_guard.get_options())
}

#[tauri::command]
pub async fn new_game(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.new_game().unwrap();
    Ok(())
}

#[tauri::command]
pub async fn stop(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.stop().unwrap();
    Ok(())
}

#[tauri::command]
pub async fn get_bestmove(state: tauri::State<'_, MyState>) -> Result<String, String> {
    let mut state_guard = state.0.lock().unwrap();
    Ok(state_guard.get_bestmove().unwrap())
}

#[tauri::command]
pub async fn go(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.go().unwrap();
    Ok(())
}

#[tauri::command]
pub async fn go_nodes(state: tauri::State<'_, MyState>, nodes: usize) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.go_nodes(nodes).unwrap();
    Ok(())
}

#[tauri::command]
pub async fn quit(state: tauri::State<'_, MyState>) -> Result<(), String> {
    let mut state_guard = state.0.lock().unwrap();
    state_guard.quit().unwrap();
    Ok(())
}
