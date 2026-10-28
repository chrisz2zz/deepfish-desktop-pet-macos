import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Runs on every desktop platform: a packaged build is preferred, but a debug
// binary works too (and is what the Windows build produces first).
function defaultBinaries() {
  if (process.platform === "win32") {
    return [
      path.join(root, "src-tauri", "target", "release", "大肥鱼桌宠.exe"),
      path.join(root, "src-tauri", "target", "release", "deepfish-desktop-pet.exe"),
      path.join(root, "src-tauri", "target", "debug", "deepfish-desktop-pet.exe")
    ];
  }
  return [
    path.join(
      root,
      "src-tauri",
      "target",
      "release",
      "bundle",
      "macos",
      "大肥鱼桌宠.app",
      "Contents",
      "MacOS",
      "deepfish-desktop-pet"
    )
  ];
}

// `DEEPFISH_BINARY` lets a debug build run the same checks without packaging.
const candidates = defaultBinaries();
// Keep the macOS result directory name that earlier builds used.
const platformLabel = { win32: "windows", darwin: "macos" }[process.platform] ?? process.platform;
const binary =
  process.env.DEEPFISH_BINARY ?? candidates.find((candidate) => fs.existsSync(candidate)) ?? candidates[0];
const output = path.join(root, "work", `smoke-${platformLabel}`);

if (!fs.existsSync(binary)) {
  throw new Error(
    `Missing ${process.platform} build at ${binary}. Run "npm run build" first, or set DEEPFISH_BINARY to a debug binary.`
  );
}

fs.rmSync(output, { recursive: true, force: true });
fs.mkdirSync(output, { recursive: true });

const exitCode = await new Promise((resolve, reject) => {
  const child = spawn(binary, [], {
    env: { ...process.env, DEEPFISH_SMOKE_DIR: output },
    stdio: "inherit"
  });
  const timeout = setTimeout(() => {
    child.kill();
    reject(new Error(`smoke test timed out on ${process.platform}`));
  }, 30000);
  child.on("exit", (code) => {
    clearTimeout(timeout);
    resolve(code);
  });
  child.on("error", (error) => {
    clearTimeout(timeout);
    reject(error);
  });
});

const pet = JSON.parse(fs.readFileSync(path.join(output, "pet.json"), "utf8"));
const settings = JSON.parse(fs.readFileSync(path.join(output, "settings.json"), "utf8"));

const expectations = [
  [exitCode === 0, `app exited with code ${exitCode}`],
  [pet.bridge === true, "pet window is missing the Tauri bridge"],
  [pet.agent === true, "pet window is missing the agent bridge"],
  [pet.markdown === true, "markdown renderer is not loaded"],
  [pet.care === true, "care reminder module is not loaded"],
  [pet.focus === true, "focus bridge is missing"],
  [pet.reminders === true, "reminder commands are not reachable"],
  [pet.reminderRoundTrip === true, "reminder create/list/remove round-trip failed"],
  [pet.openExternal === true, "link opener is missing from the bridge"],
  [pet.openExternalRejects === true, "open_external accepted a non-http url"],
  [pet.playMusic === true, "music bridge is missing"],
  [pet.playMusicRejects === true, "play_music accepted an unknown player"],
  [pet.sessions >= 0, "agent sessions could not be listed"],
  [pet.bubble === true, "chat bubble is missing from the pet window"],
  [pet.hitMask?.active > 0, "click-through mask was not reported by the renderer"],
  [pet.frameLoaded === true, "character frame did not load"],
  [pet.phase?.length > 0, "circadian phase label is empty"],
  [settings.bridge === true, "settings window is missing the Tauri bridge"],
  [settings.formVisible === true, "settings form did not render"],
  // The settings stage only toggles always-on-top, so the provider must survive
  // the round-trip whatever it was: users are free to run DeepSeek or custom.
  [settings.savedProvider === settings.provider, "settings were not persisted through the backend"],
  [settings.savedAlwaysOnTop === true, "always-on-top was not persisted"],
  [typeof settings.hasApiKey === "boolean", "api key status is missing"]
];

const failed = expectations.filter(([passed]) => !passed).map(([, message]) => message);
if (failed.length > 0) {
  console.error(`SMOKE_FAILED\n- ${failed.join("\n- ")}`);
  process.exit(1);
}

console.log(`SMOKE_OK ${path.relative(root, output)}`);
