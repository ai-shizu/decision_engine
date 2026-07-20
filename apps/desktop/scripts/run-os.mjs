#!/usr/bin/env node
/**
 * Cross-platform npm script dispatcher (no extra deps).
 * Usage: node scripts/run-os.mjs <name>
 *   win32  → powershell -File scripts/<name>.ps1
 *   else   → bash scripts/<name>.sh
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const name = process.argv[2];
if (!name || !/^[a-z0-9-]+$/i.test(name)) {
  console.error("usage: node scripts/run-os.mjs <script-name>");
  process.exit(2);
}

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const isWin = process.platform === "win32";
const scriptPath = join(scriptsDir, isWin ? `${name}.ps1` : `${name}.sh`);

if (!existsSync(scriptPath)) {
  console.error(`missing script: ${scriptPath}`);
  process.exit(1);
}

const result = isWin
  ? spawnSync(
      "powershell",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath],
      { stdio: "inherit", env: process.env },
    )
  : spawnSync("bash", [scriptPath], { stdio: "inherit", env: process.env });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
