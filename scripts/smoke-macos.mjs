import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const binary = path.join(
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
);
const output = path.join(root, "work", "smoke-macos");

if (!fs.existsSync(binary)) {
  throw new Error(`Missing macOS build at ${binary}. Run "npm run mac:build" first.`);
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
    reject(new Error("macOS smoke test timed out"));
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
  [pet.sessions >= 0, "agent sessions could not be listed"],
  [pet.bubble === true, "chat bubble is missing from the pet window"],
  [pet.hitMask?.active > 0, "click-through mask was not reported by the renderer"],
  [pet.frameLoaded === true, "character frame did not load"],
  [pet.phase?.length > 0, "circadian phase label is empty"],
  [settings.bridge === true, "settings window is missing the Tauri bridge"],
  [settings.formVisible === true, "settings form did not render"],
  [settings.savedProvider === "pollinations", "settings were not persisted through the backend"]
];

const failed = expectations.filter(([passed]) => !passed).map(([, message]) => message);
if (failed.length > 0) {
  console.error(`SMOKE_FAILED\n- ${failed.join("\n- ")}`);
  process.exit(1);
}

console.log(`SMOKE_OK ${path.relative(root, output)}`);
