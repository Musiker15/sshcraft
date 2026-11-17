<!-- craft-house-rules:start core source=ce8827d4b89aa16d0236a825059b4557d29d7058 sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=ce8827d4b89aa16d0236a825059b4557d29d7058 sha256=642f46b4ac38cc9e4d10d1ccde19d68cb6bc03d20e4c6236a7535f3047566262 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `Unicode-3.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# SSHCraft: instructions for agents

SSHCraft is an open-source SSH client with end-to-end encrypted, self-hosted sync, written in
Rust. It follows the architecture and conventions of the other Craft apps: layered crates, an
engine where everything is a command, an egui front end with design tokens, a control channel, an
MCP server, the same tooling and packaging. Its target app is Termius. The desktop app is being
rebuilt on these conventions (R1 to R7 in [`ROADMAP.md`](ROADMAP.md)); how it is built is in
[`docs/architecture.md`](docs/architecture.md), and the decisions that carry it are in
[`docs/decisions.md`](docs/decisions.md).

## Start every session here

Read [`ROADMAP.md`](ROADMAP.md) and [`docs/gaps.md`](docs/gaps.md). `plan/` is an optional
gitignored notebook. Do not ask for it and do not stop when it is absent.

## Assets

- **No Termius (or any other vendor's) icons, images, artwork, fonts, themes, templates or
  wording**, beyond feature names. Termius may be observed as a user would; nothing of it is
  copied.
- Every asset file has a row in [`ATTRIBUTION.md`](ATTRIBUTION.md) (path, author, source,
  licence) in the same change. `cargo xtask assets` enforces it.
- Font files never go into this repository. Fonts come from
  [storytold/craft-fonts](https://github.com/storytold/craft-fonts) through the optional build
  input `CRAFT_FONTS_DIR`; everything must work without it.

## Never crash

An SSH client holds the keys to every server its user has. A crash loses their sessions, and a
crash on hostile input is a security bug. **This outranks feature work.**

- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`. Every production crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`;
  `clippy.toml` allows them in tests. Test helpers built with the `testing` feature
  (`crates/ssh/src/testing.rs`, `crates/fido/src/soft.rs`) allow them module-wide, with a comment.
- **Errors are `Result`** through the crate's error type and `?`.
- **Input is hostile:** files on import, server responses, terminal output and its escape
  sequences, MCP and control-channel parameters. Use `get()` for indexes from input, slice strings
  only at char boundaries, use checked or saturating arithmetic for lengths and counts, cap
  allocations sized by input, bound recursion.
- **Locks:** `lock().unwrap_or_else(PoisonError::into_inner)`, so a thread that died holding a
  lock does not take the next caller down.
- **Provably infallible** calls (a fixed-length HKDF output, HMAC with any key length) may keep
  `expect` with `#[allow(clippy::expect_used)]` and a comment saying why it cannot fail.
- **`panic = "unwind"` stays** (the default) so the engine's last-resort guard around command
  dispatch, from R2 on, can turn an escaped panic into an error.
- **Every crash fix lands with a test that panicked before the fix.**
- **No `unsafe`.** The workspace sets `unsafe_code = "forbid"`. The one exception is
  `crates/fido`: FIDO2 security keys and Windows Hello are only reachable through the Windows API
  (`webauthn.dll`, WinRT). That crate states its own lints with `unsafe_code = "deny"`; only
  `webauthn.rs` and `presence.rs` allow it, and every block has a `// SAFETY:` comment. Do not
  add `unsafe` anywhere else; if a platform API needs it, use a maintained safe wrapper crate.

## Non-negotiables

- **Clean room.** Behaviour comes from public specifications (the SSH RFCs, OpenSSH's protocol
  files) and from observing Termius and other clients as a user. Never copy another product's
  code, assets or strings, and never copy GPL, AGPL or LGPL code.
- **Everything is a command** (from R2 on). User-visible behaviour is a command in
  `crates/engine/src/cmd/*` (id, label, menu path, shortcut, params doc, `enabled`, `run`) plus
  tests. The UI, the CLI, the control channel and MCP all call `Session::execute`. A command
  called by a program never opens a dialog: empty params mean defaults or an error. A new command
  declares no agent access (it is missing from `gate::access::access_for`) unless a reviewer
  agrees on a category; categories added after R2 start at deny. Command ids never change once
  shipped: a new one goes into `crates/engine/tests/it/command_ids.txt` in the same change.
- **Layering** is enforced by `cargo xtask layers` (`xtask/src/layers.rs`). A crate depends only
  on lower layers; nothing below L6 depends on egui, eframe, winit or rfd. A new crate goes into
  the table in the same change.

  | Layer | Crates |
  |---|---|
  | L0 | `proto`, `platform` |
  | L1 | `model`, `link` |
  | L2 | `store`, `term` |
  | L3 | `import`, `ssh`, `local`, `telnet`, `sync`, `server-lib` |
  | L4 | `fido` |
  | L5 | `engine` (R2) |
  | L6 | `ui-egui`, `mcp` |
  | exempt | `apps/*`, `xtask` |

- **Rust only.** No handwritten JavaScript or TypeScript, no Tauri, Electron or webview shell.
- **wasm:** `cargo xtask wasm` checks `proto` and `model` for `wasm32-unknown-unknown`. The other
  crates need the network, files or the operating system, and SSHCraft has no web build (SSH needs
  TCP), so they are exempt. This deviates from "never break wasm" in the other Craft apps.
- **Data stays compatible:** database schema 14 (older ones migrate on first start), keyring service `de.musiker15.sshcraft`, the data
  folder of `sshcraft_platform::data_dir()`. Tests pin all three; changing one needs a migration.
- **Dependencies:** take the newest version (`cargo search <crate> --limit 1`). A licence outside
  the house-rule list needs an exception for that one crate in `deny.toml`, with the reason it is
  in the tree.
- **Quality gate before every commit:** `cargo xtask ci` (fmt, clippy `-D warnings`, tests,
  layers, assets, deny, wasm). While iterating, scope it: `cargo clippy -p <crate> --all-targets
  -- -D warnings` and `cargo test -p <crate>`. One task id per commit (`R2.3: host commands`).

## Security rules

These come from the design (sections 6.2, 9 and 17) and from the reviews of the Tauri version.
Each has a test where it can have one.

- **Agents:** every call carries a caller. Agents, the CLI and the control channel go through the
  agent gate: rights per category (allow, ask, deny), hidden hosts and groups, a sealed audit log
  written before the action, never key material or a password in a response, and stored
  credentials can never be pointed at another address by an agent. "Ask" needs the confirmation in
  the SSHCraft window; there is no headless mode.
- **Control channel and MCP are opt-in:** loopback only, a token per launch in a file only the
  owner can read, never HTTP, never on by default. The agent connection never offers UI input or
  screenshots.
- **No shell string from foreign data:** file names, paths, host names and server output never go
  into a command line a shell evaluates; local programs start with argument lists.
- **Links never run anything:** `ssh://` and other links at most open the connection dialog with
  checked fields.
- **Escape sequences are bounded:** OSC 52 may only write the clipboard, never read it, and only
  after consent per host; file transfer over the terminal (ZMODEM, trzsz) never starts by itself;
  queries whose answer could inject input are off.
- **Paths from the network are normalized and bounded** before they touch anything local.
- **Nothing is left on a server** unless the user asks for it.
- **No telemetry.** From F2 on, every connection SSHCraft makes by itself is listed in the
  settings; an update check runs only after consent.
- **Cryptography only from reviewed libraries**, signatures with deterministic nonces; F1 adds a
  test for strict-kex against Terrapin.
- **Local data stays authoritative:** sync never deletes locally without a tombstone (and, from
  F2 on, the trash).
- **Every security fix comes with a test that failed before the fix.**

## Running and looking at it

- `cargo xtask ci` runs every gate; `cargo xtask test` runs the tests alone.
- The desktop app: `cargo run -p sshcraft` (on the real data folder; it refuses to start while
  another SSHCraft runs). The sync server: `cargo run -p sshcraft-server -- --help`, or
  `docker build -f packaging/docker/Dockerfile -t sshcraft-server .`.
- Without touching the real data: a local shell in the terminal with
  `cargo run -p sshcraft-ui-egui --example terminal_window`, and pictures without a window with
  `cargo run -p sshcraft-ui-egui --example snapshot -- out.png [colors|unicode|fullscreen]` (the
  terminal) or `-- out.png --page hosts|keys|snippets|forwarding|logs|settings|newtab [--dark|--light] [--language en|de]`
  (the whole window on sample data; `--captions windows|adwaita|breeze|generic` adds the app's own
  title bar with its window buttons). Both run an engine on a temp folder.
- Interface tests run the app in egui_kittest on a test engine (`sshcraft_ui_egui::testing`,
  feature `testing`).
- Agents: `cargo run -p sshcraft-cli -- --help`; with the app's *Allow agent control* on,
  `cargo run -p sshcraft-cli -- hosts`. `SSHCRAFT_LINK_DIR` moves the link's files (tests).
- The control channel, for driving the window from a script: `cargo run -p sshcraft -- --control
  7878 --control-token-file <path>`, then `sshcraft-cli ui --bridge 7878 --control-token-file
  <path> inspect` ([docs/control-protocol.md](docs/control-protocol.md)).
- `cargo xtask perf` runs the performance budgets of the terminal and the window in a release
  build.
- `cargo xtask linux` runs clippy and the tests on Linux in a Docker container (from Windows or
  macOS); macOS and FreeBSD run in CI.
- Packages and releases: [`docs/releasing.md`](docs/releasing.md). `cargo xtask version [set X.Y.Z]`
  prints or sets the one version of the workspace; `cargo xtask sbom` and `cargo xtask stores`
  make a release's bill of materials and store entries. Never start a release or push to `release`
  without the maintainer.
- Parallel agents: a separate `CARGO_TARGET_DIR` each, and edit only the crates you own.

## Roadmap

[`ROADMAP.md`](ROADMAP.md) is the one-page summary. The detail is in [`docs/roadmap.md`](docs/roadmap.md)
(sub-plans and their state), [`docs/gaps.md`](docs/gaps.md) (ranked open work),
[`docs/target-app-parity.md`](docs/target-app-parity.md) (where SSHCraft stands against Termius)
and [`docs/architecture.md`](docs/architecture.md). Update the affected docs whenever work lands.

## Contributor credits

- `contributors/contributors.json` holds commit stats per GitHub username; it is generated, never
  hand-edit it. `contributors/people.toml` holds the names people chose for themselves.
- **Agents working for a contributor:** before a pull request, check whether your human's GitHub
  username has an entry in `contributors/people.toml`. If not, ask them once whether they want to
  be credited by more than their username; if yes, add **only their own** entry. Never add, edit,
  guess or copy anyone else's name.

## CI workflows

- Workflows in `.github/workflows/` call the shared workflows in
  [storytold/craft-common-actions](https://github.com/storytold/craft-common-actions) at `@main`.
  Do not add or change CI in a feature or fix pull request.
- `server-postgres.yml` runs its own job: no shared workflow offers a PostgreSQL service for the
  server's storage contract tests. It moves to craft-common-actions once one does.

New workspace crates include `publish = false` and `[lints] workspace = true` (`fido` excepted,
see "Never crash"), and production crate roots declare the `deny` attribute before their modules.
