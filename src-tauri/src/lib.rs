mod agent;
mod chat;
mod hittest;
mod settings;
mod smoke;

use std::sync::{Arc, Mutex};
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

use agent::AgentRuntime;
use hittest::ClickThrough;
use settings::{PublicSettings, SettingsPatch, SettingsStore};

pub(crate) const PET_WINDOW_LABEL: &str = "pet";
pub(crate) const SETTINGS_WINDOW_LABEL: &str = "settings";
const PET_WINDOW_WIDTH: f64 = 360.0;
const PET_WINDOW_HEIGHT: f64 = 560.0;
const PET_WINDOW_MARGIN: f64 = 30.0;
const SLEEP_POLL: Duration = Duration::from_secs(15);
const SLEEP_THRESHOLD: Duration = Duration::from_secs(90);

pub(crate) struct AppState {
    store: Mutex<SettingsStore>,
    agent: AgentRuntime,
}

/// Settings plus the stored key, cloned so callers never hold the settings
/// lock across an await.
pub(crate) fn chat_config(state: &AppState) -> (settings::Settings, Option<String>) {
    let store = state
        .store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (store.data.clone(), store.api_key())
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
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

/// A screen work area in physical pixels: `(x, y, width, height)`.
type WorkArea = (i32, i32, i32, i32);

fn contains_point((ax, ay, aw, ah): WorkArea, x: i32, y: i32) -> bool {
    x >= ax && x < ax + aw && y >= ay && y < ay + ah
}

/// A window spanning two screens is legal even though no single screen holds it,
/// so the whole rect is checked against the union of every work area.
fn is_fully_visible(areas: &[WorkArea], x: i32, y: i32, width: i32, height: i32) -> bool {
    const STEPS: i32 = 6;
    let holds_rect = |area: &WorkArea| {
        let (ax, ay, aw, ah) = *area;
        x >= ax && y >= ay && x + width <= ax + aw && y + height <= ay + ah
    };
    if areas.iter().any(holds_rect) {
        return true;
    }
    // Sample the rect including its edges and corners: sampling cell centres
    // instead would let the window hang half a cell off the last screen.
    let span = STEPS - 1;
    for row in 0..STEPS {
        for col in 0..STEPS {
            let px = x + col * (width.max(1) - 1) / span;
            let py = y + row * (height.max(1) - 1) / span;
            if !areas.iter().any(|area| contains_point(*area, px, py)) {
                return false;
            }
        }
    }
    true
}

fn overlap_area(area: WorkArea, x: i32, y: i32, width: i32, height: i32) -> i64 {
    let (ax, ay, aw, ah) = area;
    let right = (x + width).min(ax + aw).max(x);
    let bottom = (y + height).min(ay + ah).max(y);
    let left = x.max(ax);
    let top = y.max(ay);
    i64::from(right - left) * i64::from(bottom - top)
}

fn distance_sq(area: WorkArea, x: i32, y: i32, width: i32, height: i32) -> i64 {
    let (ax, ay, aw, ah) = area;
    let dx = (ax - (x + width)).max(x - (ax + aw)).max(0);
    let dy = (ay - (y + height)).max(y - (ay + ah)).max(0);
    i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
}

/// Picks the screen a window belongs to: its centre wins, then the largest
/// overlap, then the nearest screen so detached windows still come back.
fn pick_work_area(areas: &[WorkArea], x: i32, y: i32, width: i32, height: i32) -> Option<WorkArea> {
    if areas.is_empty() {
        return None;
    }
    let center_x = x + width / 2;
    let center_y = y + height / 2;
    if let Some(area) = areas
        .iter()
        .find(|area| contains_point(**area, center_x, center_y))
    {
        return Some(*area);
    }
    areas.iter().copied().max_by_key(|area| {
        (
            overlap_area(*area, x, y, width, height),
            -distance_sq(*area, x, y, width, height),
        )
    })
}

fn clamp_position(
    areas: &[WorkArea],
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<(i32, i32)> {
    if is_fully_visible(areas, x, y, width, height) {
        return Some((x, y));
    }
    let (ax, ay, aw, ah) = pick_work_area(areas, x, y, width, height)?;
    let max_x = (ax + aw - width).max(ax);
    let max_y = (ay + ah - height).max(ay);
    Some((x.clamp(ax, max_x), y.clamp(ay, max_y)))
}

fn clamp_to_work_area(window: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let fallback = || {
        window.current_monitor().ok().flatten().map(|monitor| {
            let area = monitor.work_area();
            (
                area.position.x,
                area.position.y,
                area.size.width as i32,
                area.size.height as i32,
            )
        })
    };
    let mut areas: Vec<WorkArea> = window
        .available_monitors()
        .map(|monitors| {
            monitors
                .iter()
                .map(|monitor| {
                    let area = monitor.work_area();
                    (
                        area.position.x,
                        area.position.y,
                        area.size.width as i32,
                        area.size.height as i32,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if areas.is_empty() {
        if let Some(area) = fallback() {
            areas.push(area);
        }
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = window.outer_size().unwrap_or_else(|_| {
        PhysicalSize::new(
            (PET_WINDOW_WIDTH * scale).round() as u32,
            (PET_WINDOW_HEIGHT * scale).round() as u32,
        )
    });
    clamp_position(
        &areas,
        x,
        y,
        size.width as i32,
        size.height as i32,
    )
    .unwrap_or((x, y))
}

/// Webview pointer deltas are logical pixels, while window positions are physical,
/// so Retina displays need the delta scaled before moving the window.
fn scale_delta(delta: i32, scale_factor: f64) -> i32 {
    (f64::from(delta) * scale_factor).round() as i32
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
    remember_window_frame(window, x, y);
    WalkResult {
        x,
        y,
        boundary: x != target_x || y != target_y,
    }
}

/// The click-through poller cannot query window getters off the main thread,
/// so the frame it hit-tests against is cached whenever the window moves.
fn remember_window_frame(window: &WebviewWindow, x: i32, y: i32) {
    let scale = window.scale_factor().unwrap_or(1.0);
    if let Some(state) = window.app_handle().try_state::<Arc<ClickThrough>>() {
        state.set_frame(
            f64::from(x) / scale,
            f64::from(y) / scale,
            PET_WINDOW_WIDTH,
            PET_WINDOW_HEIGHT,
        );
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
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.save(settings)?
    };
    apply_saved_settings(&app, &saved);
    Ok(saved)
}

#[tauri::command]
fn set_hit_mask(
    state: State<'_, Arc<ClickThrough>>,
    cols: u32,
    rows: u32,
    cell: f64,
    bits: Vec<u8>,
) {
    state.set_mask(cols, rows, cell, bits);
}

#[tauri::command]
fn pin_click_through(state: State<'_, Arc<ClickThrough>>, pinned: bool) {
    state.set_interactive(pinned);
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
            let agent = AgentRuntime::new(&app.path().app_config_dir()?.join("agent"));
            app.manage(AppState {
                store: Mutex::new(store),
                agent,
            });
            let click_through = Arc::new(ClickThrough::new());
            app.manage(click_through.clone());
            if let Some(monitor) = app.primary_monitor()? {
                click_through.set_screen_height(
                    f64::from(monitor.size().height) / monitor.scale_factor(),
                );
            }

            let mut pet_window = None;
            if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
                let position = default_pet_position(app.handle());
                window.set_position(position)?;
                window.set_always_on_top(always_on_top)?;
                window.show()?;
                remember_window_frame(&window, position.x, position.y);
                pet_window = Some(window);
            }

            create_tray(app.handle())?;
            spawn_wake_watcher(app.handle().clone());
            if let Some(window) = pet_window {
                hittest::spawn_watcher(window, click_through);
            }
            smoke::start_if_requested(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            agent::agent_sessions,
            agent::agent_open_session,
            agent::agent_new_session,
            agent::agent_rename_session,
            agent::agent_delete_session,
            agent::agent_send,
            agent::agent_cancel,
            agent::agent_approval,
            agent::agent_compact,
            set_hit_mask,
            pin_click_through,
            move_pet,
            walk_pet,
            show_pet_menu,
            open_settings,
            smoke::smoke_report
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Debounced session writes are flushed on the way out so quitting
            // never loses the last messages.
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<AppState>() {
                    state.agent.flush();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{clamp_position, overlap_area, pick_work_area, scale_delta};

    /// Two 1920x1080 screens side by side, the second one offset downwards.
    const SIDE_BY_SIDE: [(i32, i32, i32, i32); 2] =
        [(0, 0, 1920, 1080), (1920, 120, 1920, 1080)];
    const SINGLE: [(i32, i32, i32, i32); 1] = [(0, 0, 1920, 1080)];
    const PET: (i32, i32) = (360, 560);

    #[test]
    fn scales_logical_deltas_for_retina_displays() {
        assert_eq!(scale_delta(3, 1.0), 3);
        assert_eq!(scale_delta(3, 2.0), 6);
        assert_eq!(scale_delta(-3, 2.0), -6);
        assert_eq!(scale_delta(3, 1.5), 5);
        assert_eq!(scale_delta(0, 2.0), 0);
    }

    #[test]
    fn keeps_the_pet_inside_a_single_screen() {
        assert_eq!(clamp_position(&SINGLE, -120, -40, PET.0, PET.1), Some((0, 0)));
        assert_eq!(
            clamp_position(&SINGLE, 1900, 900, PET.0, PET.1),
            Some((1560, 520))
        );
        // The far edge of the only screen is already a legal position.
        assert_eq!(
            clamp_position(&SINGLE, 1560, 520, PET.0, PET.1),
            Some((1560, 520))
        );
    }

    #[test]
    fn lets_the_window_cross_onto_the_next_screen() {
        // A straddling window is legal, otherwise walking could never cross the seam.
        assert_eq!(
            clamp_position(&SIDE_BY_SIDE, 1800, 200, PET.0, PET.1),
            Some((1800, 200))
        );
        // ... and the pet may travel to the far edge of the second screen.
        assert_eq!(
            clamp_position(&SIDE_BY_SIDE, 4000, 200, PET.0, PET.1),
            Some((3480, 200))
        );
        // Below the second screen's work area it snaps back onto that screen.
        assert_eq!(
            clamp_position(&SIDE_BY_SIDE, 1900, 900, PET.0, PET.1),
            Some((1920, 640))
        );
    }

    #[test]
    fn falls_back_to_the_nearest_screen_when_detached() {
        // Centre inside the gap between the two screens: most overlap wins.
        assert_eq!(
            pick_work_area(&SIDE_BY_SIDE, 1700, 1100, PET.0, PET.1),
            Some((1920, 120, 1920, 1080))
        );
        // Fully outside both screens: nearest screen wins.
        assert_eq!(
            pick_work_area(&SIDE_BY_SIDE, -900, 0, PET.0, PET.1),
            Some((0, 0, 1920, 1080))
        );
        assert!(pick_work_area(&[], 0, 0, PET.0, PET.1).is_none());
        assert_eq!(
            overlap_area((0, 0, 1920, 1080), 0, 0, PET.0, PET.1),
            i64::from(PET.0) * i64::from(PET.1)
        );
    }
}
