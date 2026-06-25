pub mod core;

use std::path::{Path, PathBuf};
use tauri::Manager;

#[tauri::command]
fn sync(root: String, db: String, mode: String, full: bool) -> Result<serde_json::Value, String> {
    let r = core::sync(Path::new(&root), Path::new(&db), &mode, full).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "total": r.total, "groups": r.groups, "dup_files": r.dup_files,
        "new": r.new, "pruned": r.pruned
    }))
}

#[tauri::command]
fn list_groups(db: String, mode: String, limit: usize) -> Vec<core::Group> {
    core::list_groups(Path::new(&db), &mode, limit)
}

#[tauri::command]
fn delete_files(root: String, db: String, names: Vec<String>) -> serde_json::Value {
    let (deleted, freed, errors) =
        core::delete_files(Path::new(&root), Path::new(&db), &names);
    serde_json::json!({ "deleted": deleted, "freed": freed, "errors": errors })
}

#[tauri::command]
fn stats(db: String, mode: String) -> serde_json::Value {
    let g = core::list_groups(Path::new(&db), &mode, usize::MAX);
    let dup: usize = g.iter().map(|x| x.files.len()).sum();
    serde_json::json!({ "groups": g.len(), "dup_files": dup })
}

#[tauri::command]
fn count_pngs(root: String) -> usize {
    core::count_pngs(Path::new(&root))
}

#[tauri::command]
fn default_db(app: tauri::AppHandle) -> String {
    let dir = app.path().app_data_dir().unwrap_or(PathBuf::from("."));
    dir.join("dedupe.sqlite").to_string_lossy().to_string()
}

#[tauri::command]
fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn pick_save_db(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .add_filter("sqlite", &["sqlite", "db"])
        .blocking_save_file()
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            sync,
            list_groups,
            delete_files,
            stats,
            count_pngs,
            default_db,
            pick_folder,
            pick_save_db
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
