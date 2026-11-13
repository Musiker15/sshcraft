// Builds the `sshcraft` command (MCP server and command line for AI agents) in release mode and
// puts it where the installer picks it up: src-tauri/binaries/sshcraft-<target triple>[.exe].
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(app, "..");
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"))
  .slice("host:".length)
  .trim();
execFileSync("cargo", ["build", "--release", "-p", "sshcraft-cli"], { cwd: root, stdio: "inherit" });
const ext = process.platform === "win32" ? ".exe" : "";
const target = join(app, "src-tauri", "binaries");
mkdirSync(target, { recursive: true });
copyFileSync(join(root, "target", "release", `sshcraft${ext}`), join(target, `sshcraft-${triple}${ext}`));
console.log(`sshcraft-${triple}${ext} ready`);
