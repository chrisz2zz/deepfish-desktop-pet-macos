import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const required = [
  "src/renderer/index.html",
  "src/renderer/settings.html",
  "src/renderer/app.js",
  "src/renderer/settings.js",
  "src/renderer/styles.css",
  "src/renderer/settings.css",
  "src/shared/circadian.js",
  "src/shared/behaviors.js",
  "src/shared/gesture.js",
  "src/shared/desktop-bridge.js",
  "assets/character.png",
  "assets/frames/frame-neutral.png",
  "assets/frames/frame-blink.png",
  "assets/frames/frame-walk.png",
  "assets/frames/frame-walk-b.png",
  "assets/frames/frame-ciallo-b.png",
  "assets/frames/frame-smug.png",
  "assets/frames/frame-cry.png",
  "assets/frames/frame-stranded.png",
  "assets/frames/frame-pat.png",
  "assets/frames/frame-shock.png",
  "assets/frames/frame-sleep.png",
  "assets/frames/frame-angry.png",
  "assets/frames/frame-coffee.png",
  "assets/frames/frame-feed.png",
  "assets/frames/frame-fly.png",
  "assets/frames/frame-goAway.png",
  "assets/frames/frame-hungry.png",
  "assets/frames/frame-panic.png",
  "assets/frames/frame-pressure.png",
  "assets/frames/frame-price.png",
  "assets/frames/frame-rival.png",
  "assets/frames/frame-shy.png",
  "assets/frames/frame-sit.png",
  "assets/frames/frame-startle.png",
  "assets/frames/frame-stretch.png",
  "assets/frames/frame-think.png",
  "assets/frames/frame-toy.png",
  "assets/frames/frame-trip.png",
  "assets/frames/frame-wash.png",
  "assets/frames/frame-work.png",
  "scripts/prepare-web.mjs",
  "scripts/smoke-macos.mjs",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
  "src-tauri/build.rs",
  "src-tauri/tauri.conf.json",
  "src-tauri/Info.plist",
  "src-tauri/capabilities/default.json",
  "src-tauri/src/main.rs",
  "src-tauri/src/lib.rs",
  "src-tauri/src/settings.rs",
  "src-tauri/src/chat.rs",
  "src-tauri/src/smoke.rs",
  "src-tauri/icons/icon.icns",
  "src-tauri/icons/tray.png",
];

const scripts = [
  "src/renderer/app.js",
  "src/renderer/settings.js",
  "src/shared/circadian.js",
  "src/shared/behaviors.js",
  "src/shared/gesture.js",
  "src/shared/desktop-bridge.js",
  "scripts/prepare-web.mjs",
  "scripts/smoke-macos.mjs",
];

for (const relative of required) {
  if (!fs.existsSync(path.join(root, relative))) throw new Error(`Missing required file: ${relative}`);
}

for (const relative of scripts) {
  execFileSync(process.execPath, ["--check", path.join(root, relative)], { stdio: "inherit" });
}

console.log(`Checked ${scripts.length} scripts and ${required.length} required files.`);
