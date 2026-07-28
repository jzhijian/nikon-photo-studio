// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod camera;
mod library;
mod organize;
mod presets;
mod fs;
mod db;

use tauri::Manager;

fn main() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir().expect("failed to get app data dir");
            std::fs::create_dir_all(&app_dir).ok();
            let db_path = app_dir.join("nps.db");
            let db = db::Database::open(&db_path).expect("failed to open database");
            app.manage(db);
            Ok(())
        })
        // Camera commands
        .invoke_handler(tauri::generate_handler![
            camera::commands::connect_wifi,
            camera::commands::connect_usb,
            camera::commands::disconnect,
            camera::commands::list_photos,
            camera::commands::download_photo,
            camera::commands::get_thumbnail,
            // Library commands
            library::commands::scan_folder,
            library::commands::import_folder,
            library::commands::make_thumbnail,
            library::commands::make_full_preview,
            library::commands::make_preview_fast,
            library::commands::make_thumbnails_batch,
            library::commands::nef_preview_size,
            library::commands::get_photo_info,
            library::commands::get_exif,
            // Organize commands
            organize::commands::detect_blur,
            organize::commands::batch_detect_blur,
            organize::commands::classify_photo,
            organize::commands::batch_classify,
            organize::commands::move_to_trash,
            organize::commands::restore_from_trash,
            organize::commands::archive_photos,
            // Preset commands
            presets::commands::import_preset,
            presets::commands::list_presets,
            presets::commands::apply_preset,
            presets::commands::export_with_preset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
