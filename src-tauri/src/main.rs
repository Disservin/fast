#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod process;

use std::sync::Mutex;

fn main() {
    tauri::Builder::default()
        .manage(process::MyState(Mutex::new(process::Engine {
            process: None,
            reader: None,
            clock: None,
        })))
        .invoke_handler(tauri::generate_handler![
            process::new,
            process::read_line,
            process::write,
            process::isready,
            process::uci,
            process::position_fen,
            process::position_startpos,
            process::get_bestmove,
            process::go,
            process::go_nodes,
            process::set_option,
            process::new_game,
            process::stop,
            process::quit,
        ])
        // .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
