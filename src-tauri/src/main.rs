// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod camera;
mod db;
mod diagnostics;
mod export;
mod fs;
mod library;
mod organize;
mod presets;

use tauri::Manager;

fn main() {
    if let Some(code) = diagnostics::run_if_requested() { std::process::exit(code); }
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&app_dir).ok();
            // Portable builds keep their history database beside the EXE. If
            // that directory is read-only (installed build), fall back to the
            // normal per-user application data directory.
            let portable = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|v| v.join("nikon-photo-studio.db")));
            let db_path = portable
                .filter(|p| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(p)
                        .is_ok()
                })
                .unwrap_or_else(|| app_dir.join("nikon-photo-studio.db"));
            let db = db::Database::open(&db_path).expect("failed to open database");
            app.manage(db);
            app.manage(camera::CameraState::default());
            Ok(())
        })
        // Camera commands
        .invoke_handler(tauri::generate_handler![
            camera::commands::connect_wifi,
            camera::commands::connect_usb,
            camera::preset_probe::probe_preset_usb,
            camera::sdk_probe::probe_nikon_sdk,
            camera::commands::disconnect,
            camera::commands::list_photos,
            camera::commands::download_photo,
            camera::commands::get_thumbnail,
            camera::commands::get_camera_thumbnails,
            camera::commands::prepare_camera_photo,
            camera::commands::get_camera_preview,
            camera::commands::delete_camera_photos,
            camera::commands::get_wifi_network_info,
            // Library commands
            library::commands::scan_folder,
            library::nikon_sdk::render_nikon_raw,
            library::commands::import_folder,
            library::commands::make_thumbnail,
            library::commands::make_full_preview,
            library::commands::make_preview_fast,
            library::commands::make_thumbnails_batch,
            library::commands::nef_preview_size,
            library::commands::get_photo_info,
            library::commands::get_exif,
            db::save_app_state,
            db::load_app_state,
            db::existing_paths,
            export::export_photo,
            // Organize commands
            organize::commands::detect_blur,
            organize::commands::batch_detect_blur,
            organize::commands::batch_detect_portraits,
            organize::commands::move_to_trash,
            organize::commands::restore_from_trash,
            organize::commands::archive_photos,
            // Preset commands
            presets::cloud::list_cloud_recipes,
            presets::cloud::get_cloud_recipe,
            presets::cloud::download_cloud_recipe,
            presets::commands::import_preset,
            presets::commands::inspect_nikon_file,
            presets::commands::list_presets,
            presets::commands::apply_preset,
            presets::commands::export_with_preset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
