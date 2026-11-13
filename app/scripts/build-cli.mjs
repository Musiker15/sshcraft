// Builds the `sshcraft` command (MCP server and command line for AI agents) in release mode for
// the target the app is built for, and puts it where the installer picks it up:
// src-tauri/binaries/sshcraft-<target triple>[.exe]. An empty stand-in (see build.rs) never
// reaches an installer: this script fails instead.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(app, "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"))
  .slice("host:".length)
  .trim();
// Tauri names the target it builds for; without it, it builds for this machine.
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const args = ["build", "--release", "-p", "sshcraft-cli"];
if (triple !== host) args.push("--target", triple);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const ext = triple.includes("windows") ? ".exe" : "";
const built = join(root, "target", ...(triple === host ? [] : [triple]), "release", `sshcraft${ext}`);
if (statSync(built).size === 0) throw new Error(`${built} is empty`);
const target = join(app, "src-tauri", "binaries");
mkdirSync(target, { recursive: true });
const placed = join(target, `sshcraft-${triple}${ext}`);
copyFileSync(built, placed);
if (statSync(placed).size === 0) throw new Error(`${placed} is empty`);
console.log(`sshcraft-${triple}${ext} ready`);
