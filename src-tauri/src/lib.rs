mod chat;
mod settings;
mod smoke;

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::json;
use tauri::menu::{CheckMenuItem, ContextMenu, MenuBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use chat::ChatMessage;
use settings::{PublicSettings, SettingsPatch, SettingsStore};

pub(crate) const PET_WINDOW_LABEL: &str = "pet";
pub(crate) const SETTINGS_WINDOW_LABEL: &str = "settings";
const PET_WINDOW_WIDTH: f64 = 340.0;
const PET_WINDOW_HEIGHT: f64 = 430.0;
const PET_WINDOW_MARGIN: f64 = 30.0;
const SLEEP_POLL: Duration = Duration::from_secs(15);
const SLEEP_THRESHOLD: Duration = Duration::from_secs(90);

struct AppState {
    store: Mutex<SettingsStore>,
}

#[derive(Debug, Serialize)]
struct WalkResult {
    x: i32,
    y: i32,
    boundary: bool,
}

fn pet_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PET_WINDOW_LABEL)
}

fn emit_pet_command(app: &AppHandle, command: &str) {
    if let Some(window) = pet_window(app) {
        let _ = window.emit("pet-command", command);
    }
}

fn toggle_pet(app: &AppHandle) {
    if let Some(window) = pet_window(app) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
        }
    }
}

fn save_patch(app: &AppHandle, patch: SettingsPatch) -> Option<PublicSettings> {
    let state = app.try_state::<AppState>()?;
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    store.save(patch).ok()
}

fn toggle_always_on_top(app: &AppHandle) {
    let Some(window) = pet_window(app) else {
        return;
    };
    let next = !window.is_always_on_top().unwrap_or(true);
    let _ = window.set_always_on_top(next);
    let patch = SettingsPatch {
        always_on_top: Some(next),
        ..SettingsPatch::default()
    };
    if let Some(saved) = save_patch(app, patch) {
        let _ = window.emit("settings-updated", saved);
    }
}

fn apply_saved_settings(app: &AppHandle, saved: &PublicSettings) {
    if let Some(window) = pet_window(app) {
        let _ = window.set_always_on_top(saved.settings.always_on_top);
        let _ = window.emit("settings-updated", saved.clone());
    }
}

fn clamp_to_work_area(window: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return (x, y);
    };
    let area = monitor.work_area();
    let size = window.outer_size().unwrap_or_else(|_| {
        let scale = monitor.scale_factor();
        PhysicalSize::new(
            (PET_WINDOW_WIDTH * scale).round() as u32,
            (PET_WINDOW_HEIGHT * scale).round() as u32,
        )
    });
    let max_x = (area.position.x + area.size.width as i32 - size.width as i32).max(area.position.x);
    let max_y =
        (area.position.y + area.size.height as i32 - size.height as i32).max(area.position.y);
    (
        x.clamp(area.position.x, max_x),
        y.clamp(area.position.y, max_y),
    )
}

/// Webview pointer deltas are logical pixels, while window positions are physical,
/// so Retina displays need the delta scaled before moving the window.
fn scale_delta(delta: i32, scale_factor: f64) -> i32 {
    (delta as f64 * scale_factor).round() as i32
}

fn move_by(window: &WebviewWindow, dx: i32, dy: i32) -> WalkResult {
    let Ok(position) = window.outer_position() else {
        return WalkResult {
            x: 0,
            y: 0,
            boundary: true,
        };
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let target_x = position.x + scale_delta(dx, scale);
    let target_y = position.y + scale_delta(dy, scale);
    let (x, y) = clamp_to_work_area(window, target_x, target_y);
    let _ = window.set_position(PhysicalPosition::new(x, y));
    WalkResult {
        x,
        y,
        boundary: x != target_x || y != target_y,
    }
}

fn default_pet_position(app: &AppHandle) -> PhysicalPosition<i32> {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return PhysicalPosition::new(0, 0);
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (PET_WINDOW_MARGIN * scale).round() as i32;
    let width = (PET_WINDOW_WIDTH * scale).round() as i32;
    let height = (PET_WINDOW_HEIGHT * scale).round() as i32;
    PhysicalPosition::new(
        area.position.x + area.size.width as i32 - width - margin,
        area.position.y + area.size.height as i32 - height - margin,
    )
}

pub(crate) fn create_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("settings.html".into()),
    )
    .title("大肥鱼设置")
    .inner_size(520.0, 720.0)
    .min_inner_size(440.0, 620.0)
    .build()
    .map_err(|error| format!("无法打开设置窗口：{error}"))?;

    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn popup_pet_menu(app: &AppHandle) -> Result<(), String> {
    let window = pet_window(app).ok_or_else(|| "桌宠窗口不存在".to_string())?;
    let play = SubmenuBuilder::new(app, "陪她玩")
        .text("walk", "出去散步")
        .text("wash", "让她洗碗")
        .text("coffee", "加班喝咖啡")
        .text("toy", "抱鲸鱼玩偶")
        .text("sit", "坐好")
        .text("sleep", "睡一会儿")
        .build()
        .map_err(|error| error.to_string())?;
    let always_on_top = CheckMenuItem::with_id(
        app,
        "always-on-top",
        "始终置顶",
        true,
        window.is_always_on_top().unwrap_or(true),
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let menu = MenuBuilder::new(app)
        .text("chat", "和大肥鱼聊天")
        .text("feed", "喂一碗白饭")
        .text("pat", "摸摸头")
        .item(&play)
        .separator()
        .item(&always_on_top)
        .text("settings", "设置")
        .text("hide", "暂时隐藏")
        .separator()
        .text("quit", "退出大肥鱼")
        .build()
        .map_err(|error| error.to_string())?;

    menu.popup(window.as_ref().window())
        .map_err(|error| error.to_string())
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "toggle" => toggle_pet(app),
        "chat" => emit_pet_command(app, "open-chat"),
        "settings" => {
            if let Err(error) = create_settings_window(app) {
                eprintln!("{error}");
            }
        }
        "hide" => {
            if let Some(window) = pet_window(app) {
                let _ = window.hide();
            }
        }
        "always-on-top" => toggle_always_on_top(app),
        "quit" => app.exit(0),
        command => emit_pet_command(app, command),
    }
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("toggle", "显示 / 隐藏大肥鱼")
        .text("chat", "和她聊天")
        .text("settings", "设置")
        .separator()
        .text("quit", "退出")
        .build()?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    TrayIconBuilder::with_id("pet-tray")
        .icon(icon)
        .tooltip("大肥鱼桌宠")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_pet(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

// macOS has no cross-platform sleep notification here, so a wall-clock gap is
// used to detect that the machine was asleep and greet the user on return.
fn spawn_wake_watcher(app: AppHandle) {
    thread::spawn(move || {
        let mut last = SystemTime::now();
        loop {
            thread::sleep(SLEEP_POLL);
            let now = SystemTime::now();
            let elapsed = now.duration_since(last).unwrap_or_default();
            last = now;
            if elapsed < SLEEP_THRESHOLD {
                continue;
            }
            let away_ms = elapsed.as_millis().saturating_sub(SLEEP_POLL.as_millis()) as u64;
            let action = match SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|value| value.subsec_nanos() % 2)
            {
                Ok(0) => "stretch",
                _ => "startle",
            };
            if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
                let _ = window.emit(
                    "wake-event",
                    json!({ "action": action, "reason": "resume", "awayMs": away_ms }),
                );
            }
        }
    });
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> PublicSettings {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    store.public_value()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: SettingsPatch,
) -> Result<PublicSettings, String> {
    let saved = {
        let mut store = state
            .store
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        store.save(settings)?
    };
    apply_saved_settings(&app, &saved);
    Ok(saved)
}

#[tauri::command]
async fn chat_send(
    state: State<'_, AppState>,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    let (settings, api_key) = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        (store.data.clone(), store.api_key())
    };
    if settings.provider == "deepseek" && api_key.is_none() {
        return Err("请先在设置中填写 DeepSeek API Key".to_string());
    }
    chat::chat(&settings, api_key.as_deref(), &messages).await
}

#[tauri::command]
fn move_pet(window: WebviewWindow, dx: i32, dy: i32) {
    move_by(&window, dx, dy);
}

#[tauri::command]
fn walk_pet(window: WebviewWindow, dx: i32, dy: i32) -> WalkResult {
    move_by(&window, dx, dy)
}

#[tauri::command]
fn show_pet_menu(app: AppHandle) -> Result<(), String> {
    popup_pet_menu(&app)
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    create_settings_window(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let store = SettingsStore::load(&app.path().app_config_dir()?);
            let always_on_top = store.data.always_on_top;
            app.manage(AppState {
                store: Mutex::new(store),
            });

            if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
                window.set_position(default_pet_position(app.handle()))?;
                window.set_always_on_top(always_on_top)?;
                window.show()?;
            }

            create_tray(app.handle())?;
            spawn_wake_watcher(app.handle().clone());
            smoke::start_if_requested(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            chat_send,
            move_pet,
            walk_pet,
            show_pet_menu,
            open_settings,
            smoke::smoke_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::scale_delta;

    #[test]
    fn scales_logical_deltas_for_retina_displays() {
        assert_eq!(scale_delta(3, 1.0), 3);
        assert_eq!(scale_delta(3, 2.0), 6);
        assert_eq!(scale_delta(-3, 2.0), -6);
        assert_eq!(scale_delta(3, 1.5), 5);
        assert_eq!(scale_delta(0, 2.0), 0);
    }
}
