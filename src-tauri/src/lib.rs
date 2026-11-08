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

mod commands;
mod detection;
mod engine;
mod history;
mod memory;
mod models;
mod output;
mod policy;
mod processors;
mod queue;
mod registry;
mod scheduler;
mod settings;
mod sidecar;
mod temp;

use commands::FileListState;
use history::HistoryStore;
use queue::QueueState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{LogicalPosition, LogicalSize, Manager};

pub(crate) fn setup_history<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    match HistoryStore::new(app) {
        Ok(store) => {
            let settings = commands::load_settings(app);
            let retention = settings.history.effective_retention_days();
            if let Err(e) = store.trim(retention, queue::now_ms()) {
                eprintln!("warn: startup history trim failed (retentionDays={retention}): {e}");
            }
            if !app.manage(store) {
                eprintln!("warn: HistoryStore already managed, new store discarded");
            }
        }
        Err(e) => {
            eprintln!("warn: history store unavailable, history disabled: {e}");
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacMenuItem {
    Hide,
    HideOthers,
    Separator,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Maximize,
    CloseWindow,
}

#[cfg(target_os = "macos")]
const YARD_MENU: &[MacMenuItem] = &[
    MacMenuItem::Hide,
    MacMenuItem::HideOthers,
    MacMenuItem::Separator,
    MacMenuItem::Quit,
];

#[cfg(target_os = "macos")]
const EDIT_MENU: &[MacMenuItem] = &[
    MacMenuItem::Undo,
    MacMenuItem::Redo,
    MacMenuItem::Separator,
    MacMenuItem::Cut,
    MacMenuItem::Copy,
    MacMenuItem::Paste,
    MacMenuItem::SelectAll,
];

#[cfg(target_os = "macos")]
const WINDOW_MENU: &[MacMenuItem] = &[
    MacMenuItem::Minimize,
    MacMenuItem::Maximize,
    MacMenuItem::Separator,
    MacMenuItem::CloseWindow,
];

#[cfg(target_os = "macos")]
fn macos_menu_spec() -> [(&'static str, &'static [MacMenuItem]); 3] {
    [
        ("Yard", YARD_MENU),
        ("Edit", EDIT_MENU),
        ("Window", WINDOW_MENU),
    ]
}

#[cfg(target_os = "macos")]
pub fn build_macos_menu<R: tauri::Runtime, M: tauri::Manager<R>>(
    manager: &M,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{MenuBuilder, Submenu, SubmenuBuilder};

    fn submenu<R: tauri::Runtime, M: tauri::Manager<R>>(
        manager: &M,
        title: &str,
        items: &[MacMenuItem],
    ) -> tauri::Result<Submenu<R>> {
        let mut builder = SubmenuBuilder::new(manager, title);
        for item in items {
            builder = match item {
                MacMenuItem::Hide => builder.hide(),
                MacMenuItem::HideOthers => builder.hide_others(),
                MacMenuItem::Separator => builder.separator(),
                MacMenuItem::Quit => builder.quit(),
                MacMenuItem::Undo => builder.undo(),
                MacMenuItem::Redo => builder.redo(),
                MacMenuItem::Cut => builder.cut(),
                MacMenuItem::Copy => builder.copy(),
                MacMenuItem::Paste => builder.paste(),
                MacMenuItem::SelectAll => builder.select_all(),
                MacMenuItem::Minimize => builder.minimize(),
                MacMenuItem::Maximize => builder.maximize(),
                MacMenuItem::CloseWindow => builder.close_window(),
            };
        }
        builder.build()
    }

    let [(yard_title, yard_items), (edit_title, edit_items), (window_title, window_items)] =
        macos_menu_spec();
    let app_menu = submenu(manager, yard_title, yard_items)?;
    let edit_menu = submenu(manager, edit_title, edit_items)?;
    let window_menu = submenu(manager, window_title, window_items)?;
    MenuBuilder::new(manager)
        .items(&[&app_menu, &edit_menu, &window_menu])
        .build()
}

#[cfg(all(target_os = "macos", not(test)))]
fn install_macos_menu<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let menu = build_macos_menu(app)?;
    let _previous = app.set_menu(menu)?;
    Ok(())
}

pub(crate) fn setup_hook<R: tauri::Runtime>(
    app: &mut tauri::App<R>,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let registry = std::sync::Arc::new(processors::ProcessorRegistry::new());
    if !app.manage(registry.clone()) {
        eprintln!("warn: ProcessorRegistry already managed, new registry discarded");
    }
    if !app.manage(QueueState::new(handle, registry)) {
        eprintln!("warn: QueueState already managed, new state and its worker discarded");
    }
    temp::cleanup_stale_temp_files(app.handle());
    setup_history(app.handle());
    #[cfg(all(target_os = "macos", not(test)))]
    if let Err(error) = install_macos_menu(app) {
        eprintln!("warn: macOS menu install failed; Tauri default menu remains active (native About visible): {error}");
    }
    if let Some(window) = app.get_webview_window("main") {
        restore_window_geometry(&window);
    }
    Ok(())
}

macro_rules! invoke_handlers {
    () => {
        tauri::generate_handler![
            commands::add_files,
            commands::remove_file,
            commands::clear_list,
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::compress_all,
            commands::answer_consent,
            commands::cancel_file,
            commands::cancel_all,
            commands::reveal_in_folder,
            commands::get_history,
            commands::clear_history,
            commands::open_folder,
            commands::memory_stats,
            commands::quit,
        ]
    };
}

fn window_state_flags() -> tauri_plugin_window_state::StateFlags {
    tauri_plugin_window_state::StateFlags::all()
        & !(tauri_plugin_window_state::StateFlags::DECORATIONS
            | tauri_plugin_window_state::StateFlags::SIZE
            | tauri_plugin_window_state::StateFlags::POSITION)
}

const WINDOW_GEOMETRY_FILE: &str = "window-geometry.json";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct WindowGeometry {
    width: f64,
    height: f64,
    x: f64,
    y: f64,
}

fn geometry_from_physical(
    size: tauri::PhysicalSize<u32>,
    position: tauri::PhysicalPosition<i32>,
    scale: f64,
) -> Option<WindowGeometry> {
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let geometry = WindowGeometry {
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
    };
    (geometry.width > 0.0
        && geometry.height > 0.0
        && geometry.x.is_finite()
        && geometry.y.is_finite())
    .then_some(geometry)
}

fn window_geometry_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(WINDOW_GEOMETRY_FILE))
}

fn restore_window_geometry<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    let Some(path) = window_geometry_path(window.app_handle()) else {
        return;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(geometry) = serde_json::from_slice::<WindowGeometry>(&bytes) else {
        return;
    };
    if !(geometry.width > 0.0 && geometry.height > 0.0) {
        return;
    }
    if let Err(e) = window.set_size(LogicalSize::new(geometry.width, geometry.height)) {
        eprintln!("warn: window size restore failed: {e}");
        return;
    }
    if let Err(e) = window.set_position(LogicalPosition::new(geometry.x, geometry.y)) {
        eprintln!("warn: window position restore failed: {e}");
        return;
    }
    if window.current_monitor().ok().flatten().is_none() {
        if let Err(e) = window.center() {
            eprintln!("warn: window centering fallback failed: {e}");
        }
    }
}

fn save_window_geometry<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if window.is_minimized().unwrap_or(true)
        || window.is_maximized().unwrap_or(false)
        || window.is_fullscreen().unwrap_or(false)
    {
        return;
    }
    let (Ok(scale), Ok(size), Ok(position)) = (
        window.scale_factor(),
        window.inner_size(),
        window.outer_position(),
    ) else {
        return;
    };
    let Some(geometry) = geometry_from_physical(size, position, scale) else {
        return;
    };
    let Some(path) = window_geometry_path(window.app_handle()) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec(&geometry) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!("warn: window geometry save failed: {e}");
            }
        }
        Err(e) => eprintln!("warn: window geometry encode failed: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let Some(window) = app.get_webview_window("main") else {
                eprintln!("warn: second launch: main window missing, cannot focus");
                return;
            };
            if let Err(e) = window.unminimize() {
                eprintln!("warn: second launch: unminimize failed: {e}");
            }
            if let Err(e) = window.show() {
                eprintln!("warn: second launch: show failed: {e}");
            }
            if let Err(e) = window.set_focus() {
                eprintln!("warn: second launch: set_focus failed: {e}");
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(FileListState(Mutex::new(Vec::new())))
        .setup(setup_hook)
        .invoke_handler(invoke_handlers!())
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("fatal: failed to start application: {e:#}");
            std::process::exit(1);
        })
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(window) = app.get_webview_window("main") {
                    save_window_geometry(&window);
                }
            }
        });
}

#[cfg(test)]
mod wiring_tests {
    use super::*;

    fn wiring_app(root: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_store::Builder::default().build())
            .setup(setup_hook)
            .invoke_handler(invoke_handlers!())
            .build(crate::commands::mock_test_context(
                tauri::utils::config::AppDirectoriesOverride::Root(root.to_path_buf()),
            ))
            .unwrap()
    }

    #[test]
    fn window_state_flags_leave_geometry_to_logical_persistence() {
        let flags = window_state_flags();
        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::SIZE),
            "window-state physical size cannot be restored across mixed-DPI monitors"
        );
        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::POSITION),
            "window-state physical position maps to the wrong monitor across mixed-DPI monitors"
        );
        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::DECORATIONS),
            "window-state must never restore decorations: a stale state file would re-decorate the frameless Windows window"
        );
        assert!(
            flags.contains(tauri_plugin_window_state::StateFlags::MAXIMIZED),
            "maximized state still belongs to the plugin"
        );
    }

    #[test]
    fn window_geometry_round_trips_through_logical_points() {
        let size = tauri::PhysicalSize::new(2328, 2392);
        let position = tauri::PhysicalPosition::new(3946, 298);
        assert_eq!(
            geometry_from_physical(size, position, 2.0),
            Some(WindowGeometry {
                width: 1164.0,
                height: 1196.0,
                x: 1973.0,
                y: 149.0,
            }),
            "a 2x display saved in physical pixels must persist as logical points"
        );
        assert_eq!(
            geometry_from_physical(size, position, 2.0),
            geometry_from_physical(
                tauri::PhysicalSize::new(1164, 1196),
                tauri::PhysicalPosition::new(1973, 149),
                1.0
            ),
            "the same window must persist identically on a 1x and a 2x display"
        );
        assert_eq!(geometry_from_physical(size, position, 0.0), None);
    }

    #[test]
    fn setup_hook_manages_queue_and_history_states() {
        let root = tempfile::tempdir().unwrap();
        let mut app = wiring_app(root.path());
        app.run_iteration(|_, _| {});
        assert!(
            app.try_state::<HistoryStore>().is_some(),
            "setup_hook must manage HistoryStore; deleting setup_history from the hook fails here"
        );
        assert!(app.try_state::<QueueState<tauri::test::MockRuntime>>().is_some());
    }

    #[test]
    fn registered_commands_answer_ipc_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let mut app = wiring_app(root.path());
        app.run_iteration(|_, _| {});
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let request = |cmd: &str, body: serde_json::Value| tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };

        let response = tauri::test::get_ipc_response(
            &webview,
            request(
                "get_history",
                serde_json::json!({
                    "filters": {},
                    "page": 0,
                    "pageSize": 50
                }),
            ),
        );
        let payload = response.expect("registered get_history must answer the ipc dispatch");
        let value = payload.deserialize::<serde_json::Value>().unwrap();
        assert_eq!(value["page"], 0);
        assert_eq!(value["pageSize"], 50);
        assert_eq!(value["totalSessions"], 0);

        let response = tauri::test::get_ipc_response(
            &webview,
            request("clear_history", serde_json::json!({})),
        );
        response.expect("registered clear_history must answer the ipc dispatch with Ok");
    }

    #[test]
    fn registered_quit_command_reaches_the_exit_request() {
        let root = tempfile::tempdir().unwrap();
        let mut app = wiring_app(root.path());
        app.run_iteration(|_, _| {});
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let request = |cmd: &str| tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(serde_json::json!({})),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };

        let unknown = tauri::test::get_ipc_response(&webview, request("definitely_not_a_command"));
        assert!(
            unknown.is_err(),
            "an unregistered command must fail, so the quit assertion cannot pass vacuously"
        );

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = tauri::test::get_ipc_response(&webview, request("quit"));
        }))
        .expect_err("quit must be registered: neither Ok nor a panic means it never reached request_exit");
        let msg = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>");
        assert_eq!(
            msg, "not implemented",
            "quit must panic only from MockRuntime::request_exit, whose unimplemented!() payload is \"not implemented\"; got: {msg}"
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn registered_open_folder_error_crosses_ipc_boundary() {
        let root = tempfile::tempdir().unwrap();
        let mut app = wiring_app(root.path());
        app.run_iteration(|_, _| {});
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let request = |cmd: &str, body: serde_json::Value| tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };

        let missing = root.path().join("definitely-missing-folder");
        let missing_str = missing.to_str().unwrap().to_string();
        let err = tauri::test::get_ipc_response(
            &webview,
            request("open_folder", serde_json::json!({ "path": missing_str })),
        )
        .expect_err("registered open_folder must answer the ipc dispatch");
        let err = err
            .as_str()
            .expect("open_folder error must cross the ipc boundary as a string");
        assert!(
            err.starts_with("Could not open folder "),
            "pinned open_folder error copy must survive the ipc boundary: {err}"
        );
    }

    #[test]
    fn startup_trim_failure_still_manages_history_store() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("history.jsonl")).unwrap();
        std::fs::write(
            root.path().join("settings.json"),
            serde_json::json!({ "settings": { "history": { "retentionDays": 10 } } }).to_string(),
        )
        .unwrap();

        let mut app = wiring_app(root.path());
        app.run_iteration(|_, _| {});

        assert!(
            app.try_state::<HistoryStore>().is_some(),
            "startup trim failure must not disable history for the session"
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_menu_tests {
    use super::*;

    #[test]
    fn macos_menu_has_exactly_three_submenus() {
        let spec = macos_menu_spec();

        assert_eq!(spec.len(), 3);
        assert_eq!(spec.map(|(title, _)| title), ["Yard", "Edit", "Window"]);
    }

    #[test]
    fn macos_menu_entries_match_the_spec() {
        use MacMenuItem::*;

        let spec = macos_menu_spec();

        assert_eq!(
            spec[0].1,
            &[Hide, HideOthers, Separator, Quit][..],
            "the Yard submenu must offer hide, hide others, a separator, and quit"
        );
        assert_eq!(
            spec[1].1,
            &[Undo, Redo, Separator, Cut, Copy, Paste, SelectAll][..],
            "the Edit submenu must keep undo/redo, a separator, then the clipboard items"
        );
        assert_eq!(
            spec[2].1,
            &[Minimize, Maximize, Separator, CloseWindow][..],
            "the Window submenu must offer minimize, maximize, a separator, and close"
        );
    }
}
