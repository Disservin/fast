#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[tauri::command]
fn greet() -> String {
    static mut N: i32 = 5;
    unsafe {
        N += 1;
        format!("Hello, {}!", N)
    }
}

mod process;

use process::{InnerProcessState, ProcessState};
use std::sync::Mutex;

fn main() {
    tauri::Builder::default()
        .manage(ProcessState(Mutex::new(InnerProcessState {
            process: None,
        })))
        .invoke_handler(tauri::generate_handler![
            process::start_process,
            process::send_string,
            process::end_process
        ])
        // .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
