mod builds;
mod dota;
mod gsi;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

const GSI_PORT: u16 = 38397;
const OVERLAY_LABEL: &str = "overlay";

struct Managed {
    token: String,
    port: u16,
    gsi: Option<gsi::GsiHandle>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    dota_found: bool,
    cfg_path: Option<String>,
    cfg_installed: bool,
    port: u16,
    token_prefix: String,
    data_dir: String,
    server_ok: bool,
    server_error: Option<String>,
    snapshot: gsi::Snapshot,
}

fn load_token() -> String {
    let dir = builds::data_dir();
    let path = dir.join("gsi-token.txt");

    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim().to_string();
        if text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit()) {
            return text;
        }
    }

    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, &token);
    token
}

fn toggle_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    if visible {
        let _ = window.hide();
    } else {
        place_overlay(&window);
        let _ = window.show();
        let _ = window.set_ignore_cursor_events(true);
    }
}

fn place_overlay(window: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = (370.0 * scale) as u32;
        let margin = (24.0 * scale) as i32;
        let x = size.width as i32 - width as i32 - margin;
        let y = (110.0 * scale) as i32;
        let _ = window.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
    }
}

fn create_overlay(app: &tauri::App) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("overlay.html".into()),
    )
    .title("Dota Coach Overlay")
    .inner_size(370.0, 640.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .resizable(false)
    .focused(false)
    .visible(false)
    .build()?;

    place_overlay(&window);
    let _ = window.set_ignore_cursor_events(true);
    Ok(())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Открыть настройки", true, None::<&str>)?;
    let overlay = MenuItem::with_id(app, "overlay", "Оверлей (F8)", true, None::<&str>)?;
    let data = MenuItem::with_id(app, "data", "Папка данных", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &overlay, &data, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Dota Coach")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "overlay" => toggle_overlay(app),
            "data" => {
                let _ = std::fs::create_dir_all(builds::data_dir());
                let _ = std::process::Command::new("explorer")
                    .arg(builds::data_dir())
                    .spawn();
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

#[tauri::command]
fn get_status(managed: tauri::State<'_, Managed>) -> Status {
    let cfg = dota::cfg_path();
    let snapshot = managed
        .gsi
        .as_ref()
        .and_then(|g| g.snapshot.lock().ok().map(|s| s.clone()))
        .unwrap_or_default();

    Status {
        dota_found: dota::find_cfg_dir().is_some(),
        cfg_installed: cfg.as_ref().map(|p| p.exists()).unwrap_or(false),
        cfg_path: cfg.map(|p| p.display().to_string()),
        port: managed.port,
        token_prefix: managed.token.chars().take(8).collect(),
        data_dir: builds::data_dir().display().to_string(),
        server_ok: managed.gsi.is_some(),
        server_error: managed.error.clone(),
        snapshot,
    }
}

#[tauri::command]
fn install_gsi(managed: tauri::State<'_, Managed>) -> Result<String, String> {
    let path = dota::install_gsi(&managed.token, managed.port)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn remove_gsi() -> Result<(), String> {
    dota::remove_gsi()
}

#[tauri::command]
fn get_build(hero_id: u32) -> Result<builds::HeroBuild, String> {
    builds::get_build(hero_id)
}

#[tauri::command]
fn simulate(app: AppHandle, managed: tauri::State<'_, Managed>) -> Result<gsi::Snapshot, String> {
    let gsi = managed
        .gsi
        .as_ref()
        .ok_or_else(|| managed.error.clone().unwrap_or_else(|| "GSI-сервер не запущен".to_string()))?;
    let body = gsi::simulate_body(1, 900, 1200, 480).replace("__TOKEN__", &managed.token);
    gsi.ingest(&app, &body)
}

#[tauri::command]
fn toggle_overlay_cmd(app: AppHandle) {
    toggle_overlay(&app);
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    let dir = builds::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn overlay_ready(app: AppHandle) {
    let _ = app.emit("coach://overlay-ready", ());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_overlay(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let token = load_token();
            let (gsi, error) = match gsi::start(app.handle().clone(), token.clone(), GSI_PORT) {
                Ok(handle) => (Some(handle), None),
                Err(e) => {
                    eprintln!("[gsi] {e}");
                    (None, Some(e))
                }
            };

            app.manage(Managed {
                token,
                port: GSI_PORT,
                gsi,
                error,
            });

            create_overlay(app)?;
            setup_tray(app)?;

            if let Err(e) = app
                .global_shortcut()
                .register(Shortcut::new(None, Code::F8))
            {
                eprintln!("[hotkey] F8: {e}");
            }

            if let Some(main) = app.get_webview_window("main") {
                let handle = main.clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = handle.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            install_gsi,
            remove_gsi,
            get_build,
            simulate,
            toggle_overlay_cmd,
            open_data_dir,
            overlay_ready
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
