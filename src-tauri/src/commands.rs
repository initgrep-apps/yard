// Yard — offline batch file compression
// Copyright (C) 2026 Yard Contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::history::{clamp_page_size, HistoryFilters, HistoryResponse, HistoryStore};
use crate::models::{Category, FileEntry, FileStatus};
use std::sync::{Mutex, OnceLock};
use tauri::{Manager, State};
use tauri_plugin_store::StoreExt;

pub struct FileListState(pub Mutex<Vec<FileEntry>>);

pub(crate) fn add_files_core(
    list: &mut Vec<FileEntry>,
    file_paths: Vec<String>,
    target_resolver: &dyn Fn(&Category) -> u64,
) -> Vec<FileEntry> {
    let mut new_entries = Vec::new();

    for path in file_paths {
        let p = std::path::Path::new(&path);
        if !p.is_file() {
            continue;
        }
        let canonical = match std::fs::canonicalize(p) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let canonical_str = canonical.to_string_lossy().to_string();
        if list.iter().any(|e| e.path == canonical_str) {
            continue;
        }
        let category = crate::detection::detect_category(
            std::path::Path::new(&canonical_str)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        );
        let entry = FileEntry::new(&canonical_str, target_resolver(&category));
        new_entries.push(entry.clone());
        list.push(entry);
    }

    new_entries
}

pub(crate) fn remove_file_core(list: &mut Vec<FileEntry>, file_id: &str) {
    list.retain(|e| e.id != file_id);
}

pub(crate) fn clear_list_core(list: &mut Vec<FileEntry>) {
    list.clear();
}

const STORE_FILE: &str = "settings.json";

pub fn load_settings<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> crate::settings::Settings {
    let value = match app.store(STORE_FILE) {
        Ok(store) => store.get("settings"),
        Err(e) => {
            eprintln!("warn: settings store unreadable, using defaults: {e}");
            None
        }
    };
    let corrupt = value
        .as_ref()
        .is_some_and(|v| serde_json::from_value::<crate::settings::Settings>(v.clone()).is_err());
    if corrupt {
        eprintln!("warn: corrupt settings store, falling back to defaults");
    }
    crate::settings::parse_settings_value(value)
}

fn save_settings_to_store<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &crate::settings::Settings,
) -> Result<(), String> {
    let store = match app.store(STORE_FILE) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("warn: existing settings store unavailable, creating fresh: {e}");
            app.store_builder(STORE_FILE)
                .build()
                .map_err(|e| format!("failed to build settings store: {e}"))?
        }
    };
    let value =
        serde_json::to_value(settings).map_err(|e| format!("failed to serialize settings: {e}"))?;
    store.set("settings", value);
    store
        .save()
        .map_err(|e| format!("failed to persist settings store: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_settings<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> crate::settings::Settings {
    load_settings(&app)
}

#[tauri::command]
pub fn save_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    settings: crate::settings::Settings,
) -> Result<(), String> {
    save_settings_to_store(&app, &settings)
}

#[tauri::command]
pub fn reset_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<crate::settings::Settings, String> {
    let defaults = crate::settings::Settings::default();
    save_settings_to_store(&app, &defaults)?;
    Ok(defaults)
}

#[tauri::command]
pub fn add_files<R: tauri::Runtime>(
    file_paths: Vec<String>,
    state: State<'_, FileListState>,
    app: tauri::AppHandle<R>,
) -> Vec<FileEntry> {
    let settings = load_settings(&app);
    let mut list = state.0.lock().unwrap_or_else(|e| {
        eprintln!("warn: file list mutex was poisoned, recovering: {e}");
        e.into_inner()
    });

    let resolver = |category: &Category| -> u64 {
        let key = match category {
            Category::Image => "image",
            Category::Video => "video",
            Category::Pdf => "pdf",
            Category::Document => "document",
            Category::Other => "other",
        };
        settings.target_bytes_for(key)
    };

    add_files_core(&mut list, file_paths, &resolver)
}

#[tauri::command]
pub fn remove_file(file_id: String, state: State<FileListState>) {
    let mut list = state.0.lock().unwrap_or_else(|e| {
        eprintln!("warn: file list mutex was poisoned, recovering: {e}");
        e.into_inner()
    });
    remove_file_core(&mut list, &file_id)
}

#[tauri::command]
pub fn clear_list(state: State<FileListState>) {
    let mut list = state.0.lock().unwrap_or_else(|e| {
        eprintln!("warn: file list mutex was poisoned, recovering: {e}");
        e.into_inner()
    });
    clear_list_core(&mut list)
}

#[tauri::command]
pub fn compress_all<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, FileListState>,
    queue: State<'_, crate::queue::QueueState<R>>,
    grants: Option<Vec<crate::queue::ConsentGrant>>,
) -> String {
    let list = state.0.lock().unwrap_or_else(|e| {
        eprintln!("warn: file list mutex was poisoned, recovering: {e}");
        e.into_inner()
    });
    let pending_ids: Vec<String> = list
        .iter()
        .filter(|e| e.status == FileStatus::Pending)
        .map(|e| e.id.clone())
        .collect();
    drop(list);

    // Empty batch: no work to queue — skip preflight/worker round-trip
    if pending_ids.is_empty() {
        return String::new();
    }

    let batch_id = uuid::Uuid::new_v4().to_string();
    if let Some(grants) = grants.as_deref() {
        queue.register_batch_grants(&batch_id, grants);
    }
    queue.start_batch(&app, batch_id.clone(), pending_ids);
    batch_id
}

#[tauri::command]
pub fn answer_consent<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    queue: State<'_, crate::queue::QueueState<R>>,
    request_id: String,
    allow: bool,
    scope: crate::queue::ConsentScope,
) {
    queue.answer_consent(&request_id, allow, scope);
}

#[tauri::command]
pub fn cancel_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    file_id: String,
    state: State<'_, FileListState>,
    queue: State<'_, crate::queue::QueueState<R>>,
) {
    queue.cancel_file(&file_id);
    // Not-yet-started file has no token — mark Cancelled so the worker skips it
    let cancelled = {
        let mut list = state.0.lock().unwrap_or_else(|e| {
            eprintln!("warn: file list mutex was poisoned, recovering: {e}");
            e.into_inner()
        });
        match list.iter_mut().find(|e| e.id == file_id) {
            Some(e) if e.status == FileStatus::Pending => {
                e.status = FileStatus::Cancelled;
                true
            }
            _ => false,
        }
    };
    if cancelled {
        crate::queue::emit_file_cancelled(&app, &file_id);
    }
}

#[tauri::command]
pub fn cancel_all<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    queue: State<'_, crate::queue::QueueState<R>>,
) {
    queue.cancel_all();
}

#[tauri::command]
pub fn reveal_in_folder<R: tauri::Runtime>(
    file_path: String,
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(&file_path)
        .map_err(|e| format!("Could not reveal file {}: {}", file_path, e))
}

fn warn_missing_store_get_history() {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!("warn: get_history: HistoryStore state missing, returning empty history");
    });
}

fn warn_missing_store_clear_history() {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!("warn: clear_history: HistoryStore state missing, nothing to clear");
    });
}

#[tauri::command]
pub async fn get_history<R: tauri::Runtime>(
    filters: HistoryFilters,
    page: u32,
    page_size: u32,
    app: tauri::AppHandle<R>,
) -> Result<HistoryResponse, String> {
    if app.try_state::<HistoryStore>().is_none() {
        warn_missing_store_get_history();
        return Ok(HistoryResponse::empty(page_size));
    }
    let load_app = app.clone();
    let fold = tauri::async_runtime::spawn_blocking(move || {
        load_app.state::<HistoryStore>().load()
    })
    .await
    .map_err(|e| format!("history load task failed: {e}"))??;
    Ok(crate::history::query_history(
        fold,
        &filters,
        page,
        clamp_page_size(page_size),
    ))
}

#[tauri::command]
pub fn clear_history<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let Some(store) = app.try_state::<HistoryStore>() else {
        warn_missing_store_clear_history();
        return Ok(());
    };
    store.clear()
}

#[tauri::command]
pub fn quit<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    app.exit(0);
}

#[tauri::command]
pub fn open_folder<R: tauri::Runtime>(
    path: String,
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| format!("Could not open folder {}: {}", path, e))
}

#[tauri::command]
pub fn memory_stats<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> crate::memory::MemoryStats {
    let settings = load_settings(&app);
    crate::memory::memory_stats(&settings.performance)
}

#[cfg(test)]
pub(crate) fn mock_test_context(
    dirs_override: tauri::utils::config::AppDirectoriesOverride,
) -> tauri::Context<tauri::test::MockRuntime> {
    let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
    ctx.config_mut().app.app_directories_override = Some(dirs_override);
    ctx.config_mut().identifier = "com.app.test".into();
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection;
    use crate::history::{FileResultRecord, HistoryFilters, HistoryStore, SessionStartRecord};
    use crate::models::Category;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_file(dir: &std::path::Path, name: &str) -> String {
        let p = dir.join(name);
        fs::write(&p, b"x").unwrap();
        p.to_str().unwrap().to_string()
    }

    fn default_resolver() -> impl Fn(&Category) -> u64 {
        |c| detection::default_target_bytes(*c)
    }

    #[test]
    fn test_add_files_valid_paths() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        let result = add_files_core(&mut list, vec![path], &default_resolver());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test.jpg");
        assert_eq!(result[0].category, Category::Image);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_add_files_skips_nonexistent() {
        let mut list = Vec::new();
        let result = add_files_core(
            &mut list,
            vec!["/nonexistent/path.jpg".to_string()],
            &default_resolver(),
        );
        assert!(result.is_empty());
        assert!(list.is_empty());
    }

    #[test]
    fn test_add_files_rejects_directory() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let result = add_files_core(
            &mut list,
            vec![subdir.to_str().unwrap().to_string()],
            &default_resolver(),
        );
        assert!(result.is_empty());
        assert!(list.is_empty());
    }

    #[test]
    fn test_add_files_skips_unresolvable_case_variant() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        let canonical = fs::canonicalize(&path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let case_variant = if canonical.chars().any(|c| c.is_uppercase()) {
            canonical.to_lowercase()
        } else {
            canonical.to_uppercase()
        };
        add_files_core(&mut list, vec![canonical.clone()], &default_resolver());
        add_files_core(&mut list, vec![case_variant], &default_resolver());
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_add_files_deduplicates_exact_canonical() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        let canonical = fs::canonicalize(&path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let first = add_files_core(&mut list, vec![canonical.clone()], &default_resolver());
        assert_eq!(first.len(), 1);
        let second = add_files_core(&mut list, vec![canonical], &default_resolver());
        assert!(second.is_empty(), "same canonical path twice must dedup");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_add_files_deduplicates_redundant_separators() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        add_files_core(&mut list, vec![path.clone()], &default_resolver());
        let doubled = path.replace("/test.jpg", "//test.jpg");
        let result = add_files_core(&mut list, vec![doubled], &default_resolver());
        assert!(result.is_empty(), "canonicalize normalizes // before dedup");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_add_files_stores_canonical_path() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        add_files_core(&mut list, vec![path], &default_resolver());
        let canonical = fs::canonicalize(dir.path().join("test.jpg")).unwrap();
        let stored: Vec<String> = list.iter().map(|e| e.path.clone()).collect();
        assert_eq!(stored[0], canonical.to_str().unwrap());
    }

    #[test]
    fn test_add_files_returns_new_only() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let p1 = make_file(dir.path(), "a.jpg");
        let p2 = make_file(dir.path(), "b.jpg");
        add_files_core(&mut list, vec![p1.clone()], &default_resolver());
        let result = add_files_core(&mut list, vec![p1, p2], &default_resolver());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "b.jpg");
    }

    #[test]
    fn test_add_files_empty_vec() {
        let mut list = Vec::new();
        let result = add_files_core(&mut list, vec![], &default_resolver());
        assert!(result.is_empty());
        assert!(list.is_empty());
    }

    #[test]
    fn test_remove_file_existing() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let path = make_file(dir.path(), "test.jpg");
        let entries = add_files_core(&mut list, vec![path], &default_resolver());
        remove_file_core(&mut list, &entries[0].id);
        assert!(list.is_empty());
    }

    #[test]
    fn test_clear_list() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let p1 = make_file(dir.path(), "a.jpg");
        let p2 = make_file(dir.path(), "b.jpg");
        add_files_core(&mut list, vec![p1, p2], &default_resolver());
        clear_list_core(&mut list);
        assert!(list.is_empty());
    }

    #[test]
    fn test_mutex_poison_recovery() {
        let state = FileListState(Mutex::new(Vec::new()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lock = state.0.lock().unwrap();
            panic!("intentional panic");
        }));
        assert!(result.is_err());
        let list = state.0.lock().unwrap_or_else(|e| e.into_inner());
        assert!(list.is_empty());
    }

    #[test]
    fn test_add_files_core_uses_settings_targets() {
        let dir = tempdir().unwrap();
        let mut list = Vec::new();
        let image_path = make_file(dir.path(), "photo.jpg");
        let other_path = make_file(dir.path(), "data.xyz");
        let custom = |category: &Category| -> u64 {
            match category {
                Category::Image => 500 * 1024,
                _ => 0,
            }
        };
        let result = add_files_core(&mut list, vec![image_path, other_path], &custom);
        assert_eq!(result.len(), 2);
        let image_entry = result
            .iter()
            .find(|e| e.category == Category::Image)
            .unwrap();
        assert_eq!(image_entry.target_bytes, 500 * 1024);
        let other_entry = result
            .iter()
            .find(|e| e.category == Category::Other)
            .unwrap();
        assert_eq!(other_entry.target_bytes, 0);
    }

    #[test]
    fn test_load_settings_corrupt_value_falls_back_to_defaults() {
        let corrupt = serde_json::json!({
            "outputFolder": "/tmp/out",
            "categoryTargets": { "image": { "size": "not-a-number", "unit": "KB" } }
        });
        let parsed: crate::settings::Settings =
            crate::settings::parse_settings_value(Some(corrupt));
        assert_eq!(parsed, crate::settings::Settings::default());
        assert_eq!(
            crate::settings::parse_settings_value(None),
            crate::settings::Settings::default()
        );
    }

    fn history_test_app(
        root: &std::path::Path,
        store: Option<crate::history::HistoryStore>,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(mock_test_context(
                tauri::utils::config::AppDirectoriesOverride::Root(root.to_path_buf()),
            ))
            .unwrap();
        if let Some(store) = store {
            app.manage(store);
        }
        app
    }

    fn seed_session(store: &HistoryStore, session_id: &str, started_at: i64) {
        store
            .append_session_start(&SessionStartRecord {
                session_id: session_id.into(),
                started_at,
                file_count: 1,
                output_folder: "/o".into(),
            })
            .unwrap();
    }

    #[test]
    fn page_size_clamps_helper() {
        assert_eq!(clamp_page_size(0), 50);
        assert_eq!(clamp_page_size(1), 1);
        assert_eq!(clamp_page_size(200), 200);
        assert_eq!(clamp_page_size(201), 200);
    }

    #[test]
    fn get_history_empty_response_when_store_missing() {
        let resp = HistoryResponse::empty(0);
        assert_eq!(resp.page, 0);
        assert_eq!(resp.page_size, 50);
        assert_eq!(resp.total_sessions, 0);
        assert_eq!(resp.sessions.len(), 0);
        assert_eq!(resp.summary.files, 0);
        assert_eq!(HistoryResponse::empty(500).page_size, 200);
        assert_eq!(
            crate::history::HistoryResponse::default().page_size,
            50,
            "Default must not be constructible as an invalid page_size 0"
        );
        assert_eq!(crate::history::HistoryResponse::empty(0).page_size, 50);
        assert_eq!(crate::history::HistoryResponse::empty(200).page_size, 200);
        assert_eq!(crate::history::HistoryResponse::empty(900).page_size, 200);
    }

    #[tokio::test]
    async fn get_history_over_tempdir_store_filters_and_counts_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_start(&SessionStartRecord {
                session_id: "s1".into(),
                started_at: 100,
                file_count: 1,
                output_folder: "/out".into(),
            })
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f1".into(),
                completed_at: 101,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: Some("/out/a.jpg".into()),
                category: "image".into(),
                target_bytes: 1,
                original_size: 5,
                compressed_size: Some(1),
                status: "done".into(),
                quality_used: Some(70),
                strategy_used: Some("quality=70".into()),
                error: None,
                duration_ms: 3,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(store.file_path())
            .unwrap()
            .write_all(b"garbage\n")
            .unwrap();

        let app = history_test_app(dir.path(), Some(store));
        let resp = get_history(HistoryFilters::default(), 0, 50, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(resp.total_sessions, 1);
        assert_eq!(resp.skipped_lines, 1);

        let filtered = get_history(
            HistoryFilters {
                search: Some("no-such-session".into()),
                ..HistoryFilters::default()
            },
            0,
            50,
            app.handle().clone(),
        )
        .await
        .unwrap();
        assert_eq!(filtered.total_sessions, 0);
    }

    #[tokio::test]
    async fn get_history_propagates_load_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("history.jsonl")).unwrap();
        let app = history_test_app(dir.path(), Some(HistoryStore::new_in(dir.path())));
        let err = get_history(HistoryFilters::default(), 0, 50, app.handle().clone())
            .await
            .unwrap_err();
        assert!(
            err.contains("history read failed"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn get_history_clamped_page_size_in_response() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        seed_session(&store, "s1", 0);
        let app = history_test_app(dir.path(), Some(store));

        let resp = get_history(HistoryFilters::default(), 0, 500, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(resp.page_size, 200);
        assert_eq!(resp.total_sessions, 1);

        let resp = get_history(HistoryFilters::default(), 0, 0, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(resp.page_size, 50);
    }

    #[tokio::test]
    async fn get_history_page_argument_is_clamped_and_echoed() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        seed_session(&store, "s1", 100);
        seed_session(&store, "s2", 200);
        let app = history_test_app(dir.path(), Some(store));

        let resp = get_history(HistoryFilters::default(), 9, 1, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(
            resp.page, 1,
            "page 9 with two sessions of size 1 clamps to the last page"
        );
        assert_eq!(resp.page_size, 1);
        assert_eq!(resp.total_sessions, 2);
        let ids: Vec<&str> = resp
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["s1"],
            "page 1 of two single-session pages holds the older session; the point is page 9 clamps to the last page"
        );
    }

    #[test]
    fn clear_history_removes_file_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("history.jsonl");
        let store = HistoryStore::new_in(dir.path());
        seed_session(&store, "s1", 0);
        assert!(history.exists());
        let app = history_test_app(dir.path(), Some(store));

        clear_history(app.handle().clone()).unwrap();
        assert!(!history.exists());
        clear_history(app.handle().clone()).unwrap();
    }

    #[test]
    fn clear_history_missing_store_is_noop_ok() {
        let dir = tempfile::tempdir().unwrap();
        let app = history_test_app(dir.path(), None);
        assert!(app.try_state::<HistoryStore>().is_none());

        clear_history(app.handle().clone()).unwrap();
    }

    #[test]
    fn clear_history_error_when_remove_fails() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("history.jsonl");
        fs::create_dir(&history).unwrap();
        fs::write(history.join("occupied.txt"), b"x").unwrap();
        let app = history_test_app(dir.path(), Some(HistoryStore::new_in(dir.path())));

        assert!(clear_history(app.handle().clone()).is_err());
        assert!(history.exists(), "failed clear must leave data in place");
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn open_folder_error_path() {
        let dir = tempfile::tempdir().unwrap();
        let app = history_test_app(dir.path(), None);
        let missing = dir.path().join("definitely-missing-folder");
        let missing_str = missing.to_str().unwrap().to_string();
        let expected = format!("Could not open folder {missing_str}: ");

        let err = open_folder(missing_str, app.handle().clone()).unwrap_err();
        assert!(
            err.starts_with(&expected),
            "pinned copy must name the path and keep the separator: {err}"
        );
    }

    #[test]
    fn history_store_new_resolves_under_app_data_dir() {
        let root = tempdir().unwrap();
        let app = history_test_app(root.path(), None);

        let store =
            HistoryStore::new(app.handle()).expect("app data dir must resolve under the override");
        seed_session(&store, "s1", 100);

        let path = root.path().join("history.jsonl");
        assert!(path.exists(), "store must write under app_data_dir");
        let loaded = store.load().unwrap();
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].session_id, "s1");
    }

    #[test]
    fn trim_runs_with_effective_retention_from_settings() {
        let root = tempdir().unwrap();
        let seeded = HistoryStore::new_in(root.path());
        let now = crate::queue::now_ms();
        seed_session(&seeded, "old", now - 30 * 86_400_000);
        seed_session(&seeded, "mid", now - 300 * 86_400_000);
        seed_session(&seeded, "ancient", now - 4000 * 86_400_000);
        fs::write(
            root.path().join("settings.json"),
            serde_json::json!({ "settings": { "history": { "retentionDays": 5000 } } })
                .to_string(),
        )
        .unwrap();

        let app = history_test_app(root.path(), None);

        crate::setup_history(app.handle());

        let managed = app
            .try_state::<HistoryStore>()
            .expect("setup_history must manage the store on the success path");
        let ids: Vec<String> = managed
            .load()
            .unwrap()
            .sessions
            .iter()
            .map(|s| s.session_id.clone())
            .collect();
        assert_eq!(
            ids,
            vec!["old".to_string(), "mid".to_string()],
            "effective retention 3650 trims the 4000-day session and keeps the 30- and 300-day sessions; a raw-5000 mutant keeps all three and a 90-day-default mutant keeps only the 30-day session"
        );
    }

    #[tokio::test]
    async fn missing_app_data_dir_skips_manage_and_app_still_works() {
        let root = tempfile::TempDir::new().unwrap();
        let blocker = root.path().join("blocker.txt");
        fs::write(&blocker, b"x").unwrap();

        let app = tauri::test::mock_builder()
            .build(mock_test_context(
                tauri::utils::config::AppDirectoriesOverride::Directories(
                    tauri::utils::config::AppDirectoryOverrides {
                        data: Some(blocker.join("child")),
                        ..Default::default()
                    },
                ),
            ))
            .unwrap();

        crate::setup_history(app.handle());
        assert!(app.try_state::<HistoryStore>().is_none());

        let err = match HistoryStore::new(app.handle()) {
            Ok(_) => panic!("expected app data dir creation to fail"),
            Err(e) => e,
        };
        assert!(
            err.starts_with("history store: create data dir "),
            "store construction errors are prefixed for greppability: {err}"
        );

        let resp = get_history(HistoryFilters::default(), 0, 50, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(resp.total_sessions, 0);
        assert_eq!(resp.page_size, 50);

        let resp = get_history(HistoryFilters::default(), 0, 500, app.handle().clone())
            .await
            .unwrap();
        assert_eq!(
            resp.page_size, 200,
            "missing-store echo must be the requested clamp, not a hardcoded default"
        );

        clear_history(app.handle().clone()).unwrap();
    }
}
