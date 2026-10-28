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

test("exposes the Electron-compatible API over Tauri commands", async () => {
  const tauri = fakeTauri();
  const window = loadBridge({ __TAURI__: tauri.api });

  window.deepFish.move({ x: 1.4, y: -2.6 });
  await window.deepFish.walkStep({ x: -3, y: 0 });
  await window.deepFish.getSettings();
  await window.deepFish.saveSettings({ scale: 1 });
  await window.deepFish.chat([{ role: "user", content: "你好" }]);
  await window.deepFish.showMenu();
  await window.deepFish.openSettings();

  assert.equal(
    JSON.stringify(tauri.calls.map(([command, args]) => [command, args ?? null])),
    JSON.stringify([
      ["move_pet", { dx: 1, dy: -3 }],
      ["walk_pet", { dx: -3, dy: 0 }],
      ["get_settings", null],
      ["save_settings", { settings: { scale: 1 } }],
      ["chat_send", { messages: [{ role: "user", content: "你好" }] }],
      ["show_pet_menu", null],
      ["open_settings", null]
    ])
  );
});

test("turns Tauri string rejections into Error objects", async () => {
  const tauri = fakeTauri();
  tauri.api.core.invoke = async () => {
    throw "请先在设置中填写 DeepSeek API Key";
  };
  const window = loadBridge({ __TAURI__: tauri.api });

  await assert.rejects(window.deepFish.chat([]), (error) => {
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
  await Promise.resolve();
  await Promise.resolve();

  tauri.listeners.get("pet-command")({ payload: "feed" });
  tauri.listeners.get("wake-event")({ payload: { action: "stretch", awayMs: 0 } });

  assert.deepEqual(received, ["feed", "stretch"]);
});
