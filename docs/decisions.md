# Decisions

The decisions that carry SSHCraft, newest first. The reasoning is in the design documents under
[`superpowers/specs/`](superpowers/specs/) (German).

| Date | Decision |
|---|---|
| 2026-11-14 | Database client as a separate sister app after V1; it reaches server databases through a tunnel SSHCraft opens for it. |
| 2026-11-14 | `fido` is the one crate that may use `unsafe` (WebAuthn and Windows Hello through the Windows API). |
| 2026-11-14 | Agents get no headless mode: the confirmation and Windows Hello need the window, and only one process opens the database. |
| 2026-11-14 | New agent categories start at "deny"; secrets, imports from agent-given paths and the agent rights themselves are never reachable for agents. |
| 2026-11-14 | wasm is checked for `proto` and `model` only; there is no web build. |
| 2026-11-14 | Licence MIT OR Apache-2.0 instead of AGPL-3.0-only; versions up to `tauri-final` stay under AGPL. |
| 2026-11-14 | Rebuild on the Craft conventions: Rust and egui instead of Tauri and React, a new workspace, the core crates moved in. The Tauri version is kept at the tag `tauri-final`. |
| 2026-11-14 | SFTP joins the plan, after the first release. |
| 2026-11-13 | Session logs record output only, never keystrokes; recording is off by default; log sync has its own switch and stays in the personal area. |
| 2026-11-13 | AI agents through MCP and a command line: rights per category, confirmation only in the SSHCraft window, hidden hosts, a sealed audit log written before the action, never key material. |
| 2026-11-12 | The feature set of Termius, improved, with SFTP later; English and German with AUTO. |
| 2026-11-11 | The sync server never reads user data, not even after a takeover: end-to-end encryption, signed records, fingerprint comparison of account keys, rollback protection. |
| 2026-11-11 | The account key is created by the client and kept in Proton Pass; every device has its own key and can be revoked on its own. |
| 2026-11-11 | A changed server fingerprint always aborts the connection, also on reconnect. |
