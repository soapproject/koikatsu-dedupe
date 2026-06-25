pub mod core;

use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

// Heavy commands are async + run on a blocking worker so they never block the
// main (UI) thread. `sync` streams progress via the "sync-progress" event.

#[tauri::command]
async fn sync(
    app: tauri::AppHandle,
    root: String,
    db: String,
    mode: String,
    full: bool,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut on = |p: core::Progress| {
            let _ = app.emit("sync-progress", p);
        };
        let r = core::sync(Path::new(&root), Path::new(&db), &mode, full, &mut on)
            .map_err(|e| e.to_string())?;
        Ok::<serde_json::Value, String>(serde_json::json!({
            "total": r.total, "groups": r.groups, "dup_files": r.dup_files,
            "new": r.new, "pruned": r.pruned
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_groups(db: String, mode: String, limit: usize) -> Vec<core::Group> {
    tauri::async_runtime::spawn_blocking(move || core::list_groups(Path::new(&db), &mode, limit))
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn delete_files(root: String, db: String, names: Vec<String>) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (deleted, freed, errors) =
            core::delete_files(Path::new(&root), Path::new(&db), &names);
        serde_json::json!({ "deleted": deleted, "freed": freed, "errors": errors })
    })
    .await
    .unwrap_or_else(|e| serde_json::json!({ "deleted": 0, "freed": 0, "errors": [e.to_string()] }))
}

#[tauri::command]
async fn stats(db: String, mode: String) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || {
        let g = core::list_groups(Path::new(&db), &mode, usize::MAX);
        let dup: usize = g.iter().map(|x| x.files.len()).sum();
        let synced = core::mode_hashed(Path::new(&db), &mode);
        serde_json::json!({ "groups": g.len(), "dup_files": dup, "synced": synced })
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({ "groups": 0, "dup_files": 0 }))
}

#[tauri::command]
async fn card_strings(path: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || core::card_strings(Path::new(&path)))
        .await
        .unwrap_or_default()
}

/// Copy a card into the game's chara folder so the user can load it in-game.
#[tauri::command]
async fn copy_to_game(src: String, dest_dir: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let src = PathBuf::from(&src);
        let name = src.file_name().ok_or_else(|| "來源路徑無檔名".to_string())?;
        let dest = PathBuf::from(&dest_dir);
        if !dest.is_dir() {
            return Err(format!("遊戲 chara 路徑不存在或不是資料夾：{}", dest.display()));
        }
        let to = dest.join(name);
        std::fs::copy(&src, &to).map_err(|e| format!("{} → {}：{}", src.display(), to.display(), e))?;
        Ok(to.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn count_pngs(root: String) -> usize {
    tauri::async_runtime::spawn_blocking(move || core::count_pngs(Path::new(&root)))
        .await
        .unwrap_or(0)
}

#[tauri::command]
fn default_db(app: tauri::AppHandle) -> String {
    let dir = app.path().app_data_dir().unwrap_or(PathBuf::from("."));
    dir.join("dedupe.sqlite").to_string_lossy().to_string()
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
}

#[tauri::command]
async fn pick_save_db(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog()
            .file()
            .add_filter("sqlite", &["sqlite", "db"])
            .blocking_save_file()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
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
            card_strings,
            copy_to_game,
            count_pngs,
            default_db,
            pick_folder,
            pick_save_db
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
