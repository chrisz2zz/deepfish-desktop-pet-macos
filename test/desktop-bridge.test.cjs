const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const bridgeSource = fs.readFileSync(
  path.join(__dirname, "..", "src", "shared", "desktop-bridge.js"),
  "utf8"
);

function fakeTauri() {
  const calls = [];
  const listeners = new Map();
  return {
    calls,
    listeners,
    api: {
      core: {
        invoke: async (...args) => {
          calls.push(args);
          return undefined;
        }
      },
      event: {
        listen: async (event, handler) => {
          listeners.set(event, handler);
          return () => listeners.delete(event);
        }
      }
    }
  };
}

function loadBridge(window) {
  vm.runInNewContext(bridgeSource, { window });
  return window;
}

test("exposes the desktop bridge over Tauri commands", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });

  window.deepFish.move({ x: 1.4, y: -2.6 });
  await window.deepFish.walkStep({ x: -3, y: 0 });
  await window.deepFish.getSettings();
  await window.deepFish.saveSettings({ scale: 1 });
  await window.deepFish.agent.openSession("s1");
  await window.deepFish.agent.send("s1", "你好", 7);
  await window.deepFish.showMenu();
  await window.deepFish.openSettings();

  assert.equal(
    JSON.stringify(tauri.calls.map(([command, args]) => [command, args ?? null])),
    JSON.stringify([
      ["move_pet", { dx: 1, dy: -3 }],
      ["walk_pet", { dx: -3, dy: 0 }],
      ["get_settings", null],
      ["save_settings", { settings: { scale: 1 } }],
      ["agent_open_session", { sessionId: "s1" }],
      ["agent_send", { sessionId: "s1", requestId: 7, text: "你好" }],
      ["show_pet_menu", null],
      ["open_settings", null]
    ])
  );
});

test("exposes session and context management commands", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });

  await window.deepFish.agent.listSessions();
  await window.deepFish.agent.newSession("新标题");
  await window.deepFish.agent.renameSession("s1", "改名");
  await window.deepFish.agent.compact("s1");
  await window.deepFish.agent.cancel("s1");
  await window.deepFish.agent.approve("call_9", false);
  await window.deepFish.agent.deleteSession("s1");

  assert.equal(
    JSON.stringify(tauri.calls.map(([command, args]) => [command, args ?? null])),
    JSON.stringify([
      ["agent_sessions", null],
      ["agent_new_session", { title: "新标题" }],
      ["agent_rename_session", { sessionId: "s1", title: "改名" }],
      ["agent_compact", { sessionId: "s1" }],
      ["agent_cancel", { sessionId: "s1" }],
      ["agent_approval", { callId: "call_9", allow: false }],
      ["agent_delete_session", { sessionId: "s1" }]
    ])
  );
});

test("defaults the session arguments to null", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });

  await window.deepFish.agent.openSession();
  await window.deepFish.agent.newSession();

  assert.equal(
    JSON.stringify(tauri.calls.map(([command, args]) => [command, args])),
    JSON.stringify([
      ["agent_open_session", { sessionId: null }],
      ["agent_new_session", { title: null }]
    ])
  );
});

test("turns Tauri string rejections into Error objects", async () => {
  const tauri = fakeTauri();
  tauri.api.core.invoke = async () => {
    throw "请先在设置中填写 DeepSeek API Key";
  };
  const window = loadBridge({ __TAURI__: tauri.api });

  await assert.rejects(window.deepFish.agent.send("s1", "你好", 1), (error) => {
    // The bridge runs in the page realm, so check the shape rather than the host prototype.
    assert.equal(Object.prototype.toString.call(error), "[object Error]");
    assert.equal(error.message, "请先在设置中填写 DeepSeek API Key");
    return true;
  });
});

test("keeps the preload bridge when Electron already provided one", () => {
  const preloadApi = { move: () => "electron" };
  const window = loadBridge({ __TAURI__: fakeTauri().api, deepFish: preloadApi });

  assert.equal(window.deepFish, preloadApi);
});

test("forwards Tauri events to renderer callbacks", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });
  const received = [];

  window.deepFish.onCommand((command) => received.push(command));
  window.deepFish.onWake((event) => received.push(event.action));
  window.deepFish.agent.onEvent((event) => received.push(event.type));
  window.deepFish.onSpeak((event) => received.push(event.text));
  await Promise.resolve();
  await Promise.resolve();

  tauri.listeners.get("pet-command")({ payload: "feed" });
  tauri.listeners.get("wake-event")({ payload: { action: "stretch", awayMs: 0 } });
  tauri.listeners.get("agent-event")({ payload: { type: "delta", text: "嗨" } });
  tauri.listeners.get("pet-speak")({ payload: { text: "好的" } });

  assert.deepEqual(received, ["feed", "stretch", "delta", "好的"]);
});

test("forwards click-through updates to the backend", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });

  window.deepFish.setHitMask({ cols: 2, rows: 2, cell: 8, bits: [3, 0] });
  window.deepFish.pinClickThrough(true);
  await Promise.resolve();

  assert.equal(
    JSON.stringify(tauri.calls.map(([command, args]) => [command, args ?? null])),
    JSON.stringify([
      ["set_hit_mask", { cols: 2, rows: 2, cell: 8, bits: [3, 0] }],
      ["pin_click_through", { pinned: true }]
    ])
  );
});
