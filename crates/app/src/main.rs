// The desktop shell (ADR 0002). All data work happens in Rust; the webview is
// a display client that only ever receives cursor-filtered data (ADR 0003).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod review;
mod settings;
mod state;

fn main() {
    let root = replay_data::paths::root();
    let app_state = match state::AppState::new(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bar-replay could not start: {e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::instruments,
            commands::coverage,
            commands::fetch_range,
            commands::list_sessions,
            commands::create_session,
            commands::open_session,
            commands::set_timeframe,
            commands::view,
            commands::step_forward,
            commands::step_back,
            commands::jump_to,
            commands::place_order,
            commands::modify,
            commands::cancel_order,
            commands::close_position,
            review::review,
            review::add_note,
            review::export_session,
            settings::has_api_key,
            settings::providers,
            settings::set_api_key,
            settings::clear_api_key,
            settings::available_instruments,
            settings::pick_csv,
            settings::import_csv,
            settings::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("starting the Bar Replay window");
}
