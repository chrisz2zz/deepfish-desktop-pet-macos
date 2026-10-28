import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "build", "web");

// Tauri serves this folder as the web root. Keeping the same relative layout the
// Electron build uses means `../shared` and `../../assets` resolve from any page.
const copies = [
  ["src/renderer", "."],
  ["src/shared", "shared"],
  ["assets", "assets"]
];

fs.rmSync(output, { recursive: true, force: true });
fs.mkdirSync(output, { recursive: true });
for (const [from, to] of copies) {
  fs.cpSync(path.join(root, from), path.join(output, to), { recursive: true });
}

console.log(`Prepared ${path.relative(root, output)} for Tauri`);
