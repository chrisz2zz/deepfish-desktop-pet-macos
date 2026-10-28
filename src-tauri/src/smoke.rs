use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{create_settings_window, PET_WINDOW_LABEL, SETTINGS_WINDOW_LABEL};

pub const SMOKE_DIR_ENV: &str = "DEEPFISH_SMOKE_DIR";

static COMPLETE: AtomicBool = AtomicBool::new(false);

const PET_CHECK: &str = r##"(async () => {
  // Give the preloaded frames a moment to land so the click-through mask is built.
  await new Promise((resolve) => setTimeout(resolve, 900));
  const character = document.querySelector("#character");
  const stage = document.querySelector("#pet-stage");
  const root = document.querySelector("#pet-app");
  const frames = performance.getEntriesByType("resource").filter((entry) => entry.name.includes("/assets/frames/"));
  window.__TAURI__.core.invoke("smoke_report", { stage: "pet", data: {
    bridge: typeof window.deepFish?.walkStep === "function",
    agent: typeof window.deepFish?.agent?.send === "function",
    markdown: typeof window.DeepFishMarkdown?.render === "function",
    care: typeof window.DeepFishCare?.dueType === "function",
    focus: typeof window.deepFish?.onFocus === "function",
    reminders:
      await window.deepFish
        ?.listReminders?.()
        .then((items) => Array.isArray(items), () => false),
    reminderRoundTrip: await (async () => {
      const created = await window.__TAURI__.core.invoke("add_reminder", { text: "冒烟测试", minutes: 5 });
      const listed = await window.__TAURI__.core.invoke("list_reminders");
      const removed = await window.__TAURI__.core.invoke("remove_reminder", { id: created?.id });
      return Boolean(created?.id) && listed.some((item) => item.id === created.id) && removed === true;
    })().catch(() => false),
    openExternal: typeof window.deepFish?.openExternal === "function",
    openExternalRejects: await window.__TAURI__.core
      .invoke("open_external", { url: "file:///etc/passwd" })
      .then(() => false, () => true),
    playMusic: typeof window.deepFish?.playMusic === "function",
    playMusicRejects: await window.__TAURI__.core
      .invoke("play_music", { query: "", site: "netease", player: "bogus" })
      .then(() => false, () => true),
    sessions: (await window.deepFish?.agent?.listSessions?.().catch(() => []))?.length ?? 0,
    bubble: Boolean(document.querySelector("#chat-panel")?.classList.contains("chat-panel")),
    hitMask: window.deepFishHitMask?.() ?? null,
    phase: document.querySelector("#phase-label")?.textContent ?? "",
    frame: character?.dataset.frame ?? null,
    frameLoaded: Boolean(character?.complete && character.naturalWidth > 0),
    stage: stage ? { width: stage.getBoundingClientRect().width, height: stage.getBoundingClientRect().height } : null,
    scale: stage?.style.transform ?? "",
    frameResources: frames.length
  } });
})();"##;

const SETTINGS_CHECK: &str = r##"(async () => {
  const saved = await window.__TAURI__.core.invoke("save_settings", { settings: { alwaysOnTop: true } });
  await window.__TAURI__.core.invoke("smoke_report", { stage: "settings", data: {
    bridge: typeof window.deepFish?.saveSettings === "function",
    provider: document.querySelector('input[name="provider"]:checked')?.value ?? null,
    endpoint: document.querySelector("#endpoint")?.value ?? null,
    model: document.querySelector("#model")?.value ?? null,
    scale: document.querySelector("#scale-output")?.textContent ?? null,
    formVisible: Boolean(document.querySelector("#settings-form")),
    savedProvider: saved.provider,
    savedScale: saved.scale,
    savedAlwaysOnTop: saved.alwaysOnTop,
    hasApiKey: saved.hasApiKey
  } });
})();"##;

pub fn start_if_requested(app: AppHandle) {
    let Ok(directory) = std::env::var(SMOKE_DIR_ENV) else {
        return;
    };
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1800));

        let Some(pet) = app.get_webview_window(PET_WINDOW_LABEL) else {
            fail(&app, "missing pet window");
            return;
        };
        if let Err(error) = pet.eval(PET_CHECK) {
            fail(&app, &format!("pet eval failed: {error}"));
            return;
        }

        thread::sleep(Duration::from_millis(1500));
        if let Err(error) = create_settings_window(&app) {
            fail(&app, &format!("settings window failed: {error}"));
            return;
        }

        thread::sleep(Duration::from_millis(1800));
        if let Some(settings) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
            if let Err(error) = settings.eval(SETTINGS_CHECK) {
                fail(&app, &format!("settings eval failed: {error}"));
                return;
            }
        } else {
            fail(&app, "missing settings window");
            return;
        }

        thread::sleep(Duration::from_millis(6000));
        if !COMPLETE.load(Ordering::SeqCst) {
            eprintln!("SMOKE_FAILED timeout {directory}");
            app.exit(1);
        }
    });
}

fn fail(app: &AppHandle, reason: &str) {
    eprintln!("SMOKE_FAILED {reason}");
    app.exit(1);
}

#[tauri::command]
pub fn smoke_report(app: AppHandle, stage: String, data: serde_json::Value) -> Result<(), String> {
    let directory = std::env::var(SMOKE_DIR_ENV).map_err(|_| "冒烟模式未启用".to_string())?;
    let path = PathBuf::from(directory).join(format!("{stage}.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| error.to_string())?;

    if stage == "settings" {
        COMPLETE.store(true, Ordering::SeqCst);
        app.exit(0);
    }
    Ok(())
}
