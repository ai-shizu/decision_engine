mod commands;
mod engine;
mod paths;

use std::sync::Arc;

use engine::EngineManager;
use tauri::RunEvent;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = EngineManager::new();
    let engine_for_exit = Arc::clone(&engine);

    tauri::Builder::default()
        .manage(Arc::clone(&engine))
        .invoke_handler(tauri::generate_handler![
            commands::pkb_invoke,
            commands::pkb_engine_ready,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let manager = Arc::clone(&engine);

            tauri::async_runtime::spawn(async move {
                if let Err(err) = manager.start(handle).await {
                    eprintln!("PKB engine startup failed: {err}");
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_, event| {
            if matches!(event, RunEvent::Exit) {
                engine_for_exit.shutdown();
            }
        });
}
