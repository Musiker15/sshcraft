# SSHCraft Teilplan R1: Gerüst und Regeln Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Das Repository steht nach den Konventionen der Craft-Apps: ein geschichteter Workspace mit den umgezogenen Kern-Crates (L0 bis L4) und dem Sync-Server als App, Lizenz MIT OR Apache-2.0, `AGENTS.md` mit den House rules, `cargo xtask ci` als Tor. Die Tauri-Oberfläche, `sshcraft-agent` und `sshcraft-cli` sind entfernt; die Desktop-App entsteht ab R2.

**Architecture:**
- **Ausgangspunkt:** Tag `tauri-final` auf `38ab865` hält die Tauri-Fassung fest. R1 arbeitet nur mit dem, was bleibt: Kern, Protokoll, SSH, lokal, Sync, Server, FIDO.
- **Aufteilung von `sshcraft-core`** nach der Abhängigkeit der Module: reine Daten nach `model` (L1), alles mit SQLite, Tresor und Schlüsselbund nach `store` (L2), Importe und Export nach `import` (L3); `authorized_keys` zieht nach `ssh`, weil nur `ssh` es nutzt. Neu ist `platform` (L0) für den Datenordner.
- **Werkzeuge:** `xtask` nach dem Muster von gridcraft (`ci`, `layers`, `assets`, `deny`, `wasm`), die CI ruft die geteilten Workflows aus `storytold/craft-common-actions`.

**Tech Stack:** Rust 1.99 (`rust-version = "1.95"`, Edition 2024), cargo-deny 0.20.2, cargo-nextest 0.9.152, serial2 0.2.39 (statt serialport), Ziel `wasm32-unknown-unknown`.

**Spec:** `docs/superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`, Abschnitte 4, 5, 8, 11, 12, 13 (Zeile R1), 14 und 17 (soweit R1 betroffen).

## Voraussetzungen

Werkzeuge, die vor Task 5 installiert sein müssen. Wer sie installiert, schreibt es Moritz ausdrücklich in den Chat.

- `cargo install cargo-deny --version 0.20.2 --locked`
- `cargo install cargo-nextest --version 0.9.152 --locked`
- Ziel `wasm32-unknown-unknown`: auf dieser Maschine schon installiert (`rustup target list --installed`).

## Global Constraints

- **Arbeitsbaum** `G:\Claude\worktrees\sshcraft-r1`, Zweig `r1-geruest` von `main`. Nach `main` nur über einen Pull Request (House rule 8, Regelsatz auf GitHub). Dieser Pfad steht nur in diesem Plan, nie in einer Datei des Repositorys (House rule 13).
- **Sprache:** Code, Kommentare, Doku und Commit-Nachrichten im Repository englisch, eine Aufgaben-Id je Commit (`R1.3: xtask layers and assets`). Kein `Co-Authored-By`, kein KI-Hinweis in Commits und Pull Requests (Moritz' wichtigste Regel, House rule 15). Dieser Plan bleibt deutsch.
- **Workspace:** `resolver = "3"`, `members = ["crates/*", "apps/*", "xtask"]`, `[workspace.package] version = "0.1.0"`, `edition = "2024"`, `license = "MIT OR Apache-2.0"`, `rust-version = "1.95"`, `repository = "https://github.com/musiker15/sshcraft"`.
- **Namen:** Ordner `crates/<name>`, Paket `sshcraft-<name>`; Apps unter `apps/<paket>`.
- **Lints:** `[workspace.lints.rust] unsafe_code = "forbid"`; `[workspace.lints.clippy]` `cognitive_complexity = "deny"`, `too_many_arguments = "deny"`, `wildcard_imports = "deny"`, `dbg_macro = "warn"`, `todo = "warn"`. Jedes Crate `[lints] workspace = true`, außer `fido` (Task 4). Jede Produktions-Crate-Wurzel trägt wörtlich `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- **Profile:** `panic = "abort"` fällt weg (Vorgabe `unwind` bleibt). Die bisherigen `opt-level = 3` für `curve25519-dalek`, `ed25519-dalek`, `sha2` im Dev-Profil bleiben.
- **Daten bleiben gleich:** Schema 11, Schlüsselbund-Dienst `de.musiker15.sshcraft`, Datenordner `ProjectDirs::from("de", "musiker15", "SSHCraft").data_dir()` mit `sshcraft.db`. Kein Verhalten der umgezogenen Crates ändert sich, außer dem Entfernen von Panics (Task 4) und dem Wechsel auf serial2 (Task 5).
- **Abhängigkeiten:** vor dem Eintragen die neueste Fassung mit `cargo search <crate> --limit 1` prüfen und nehmen (Moritz, 18.09.2026). Vor jeder Korrektur zuerst prüfen, ob ein Update des beteiligten Pakets den Fehler schon behebt (Moritz, 09.11.2026).
- **Anhalten und fragen** (House rule 9) bei: einer Lizenz außerhalb der Liste, die sich nicht mit einer Ausnahme je Crate nach Spec Abschnitt 14 lösen lässt, einer Sicherheitsmeldung (RUSTSEC) ohne behobene Fassung, Geheimnissen, allem, was schwer rückgängig zu machen ist.
- **Gegenproben** (Moritz' Regel 4) für Tests, die eine Regel, ein Recht oder einen Datenweg prüfen: Fehler einbauen, Test wird rot, Fehler zurück.
- Jede Aufgabe endet mit grünem `cargo xtask ci`; Tasks 1 bis 4, bevor es `xtask` gibt, mit `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` und `cargo test --workspace`.

## Review Focus

1. **Bestehende Daten nach der Aufteilung:** Wer die Tauri-Fassung benutzt hat, hat seine Datenbank unter dem alten Pfad und den Hauptschlüssel unter dem alten Dienstnamen. Erwartet: `platform::data_dir()` liefert genau den alten Ordner, `store` öffnet eine Datenbank in Schema 11 ohne Migration, `vault` sucht unter `de.musiker15.sshcraft` (Tests in Task 2).
2. **Vergiftete Sperren:** Ein Thread stirbt, während er eine Sperre in `tunnel.rs` oder `pty.rs` hält. Erwartet: Weiterleitung und lokales Terminal arbeiten weiter, kein Folge-Panic (Tests in Task 6).
3. **Serielle Einstellungen nach dem Bibliothekswechsel:** Baudrate, Datenbits, Stoppbits, Parität und Flusskontrolle kommen unverändert an; ungültige Werte aus der Oberfläche liefern einen Fehler statt eines Panics oder stiller Vorgaben (Tests in Task 4).
4. **Neues Crate ohne Schicht:** Jemand legt `crates/foo` an und trägt es nicht in die Schichtentabelle ein. Erwartet: `cargo xtask layers` schlägt fehl und nennt das Crate (Test in Task 5).
5. **Reste der alten Lizenz:** Irgendwo im Baum steht noch AGPL (README, `Cargo.toml` eines Crates, Doku). Erwartet: außer in `docs/superpowers/` kein Treffer (Prüfschritt in Task 7).

## Dateistruktur nach R1

```text
apps/sshcraft-server/          main.rs aus crates/sshcraft-server/src/main.rs
crates/proto/                  sshcraft-proto      L0  unverändert
crates/platform/               sshcraft-platform   L0  neu: data_dir, home_dir
crates/model/                  sshcraft-model      L1  model, forward, keytypes, policy, logtext, os_detect
crates/store/                  sshcraft-store      L2  store (+credential_store, share_store, sync_store), vault, session_log, agent, history, snippets
crates/import/                 sshcraft-import     L3  import/*, sshconfig, export
crates/ssh/                    sshcraft-ssh        L3  bisher + authorized_keys
crates/local/                  sshcraft-local      L3  pty, serial (serial2)
crates/sync/                   sshcraft-sync       L3  unverändert bis auf Abhängigkeiten
crates/server/                 sshcraft-server-lib L3  Bibliothek des Sync-Servers
crates/fido/                   sshcraft-fido       L4  bisher + presence (Windows Hello aus app/src-tauri/src/hello.rs)
xtask/                         ci, layers, assets, deny, wasm
packaging/docker/Dockerfile    aus crates/sshcraft-server/Dockerfile
AGENTS.md CLAUDE.md README.md ROADMAP.md SECURITY.md GLOSSARY.md ATTRIBUTION.md NOTICE LICENSE-MIT LICENSE-APACHE
clippy.toml rustfmt.toml deny.toml .cargo/config.toml .config/nextest.toml .coderabbit.yaml
.github/workflows/{ci.yml, house-rules.yml, server-postgres.yml} .github/dependabot.yml
contributors/people.toml contributors/contributors.json
docs/{architecture.md, decisions.md, gaps.md, roadmap.md, target-app-parity.md} + bestehende Doku
```

Abweichungen von der Tabelle in Spec Abschnitt 5, beide nach dessen Bedingungen:

- `import` liegt auf L3, nicht L2: Es nutzt `store` (L2), und die Schichtenregel erlaubt nur tiefere Schichten. `history` und `snippets` liegen in `store`, nicht in `model`, weil sie Datenbank und Tresor brauchen.
- Die Bibliothek des Servers heißt `sshcraft-server-lib`, weil die App `apps/sshcraft-server` den Paketnamen `sshcraft-server` und damit den Programmnamen behält (Docker-Image, Doku). In der Schichtentabelle heißt sie `server-lib`.

Nicht in R1: `sonar-project.properties` (SonarCloud braucht ein Projekt, sinnvoll erst mit öffentlichem Repository), `supply-chain.yml`, `packaging/` außer Docker, `version`, `stats` und `parity` in `xtask` (R7 und später).

---

### Task 1: Ausgangspunkt, Lizenz, Workspace

**Files:** Delete `app/`, `crates/sshcraft-agent/`, `crates/sshcraft-cli/`, `LICENSE`; Move `crates/sshcraft-<x>/` nach `crates/<x>/` für proto, ssh, local, sync, fido (`git mv`), `crates/sshcraft-core/` bleibt bis Task 2, `crates/sshcraft-server/` bis Task 3; Create `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`; Modify `Cargo.toml`, alle Crate-`Cargo.toml` (`license.workspace = true`), `.gitignore`.

**Interfaces, Produces:** Workspace mit Glob-Mitgliedern; Paketnamen unverändert (`sshcraft-proto`, `sshcraft-ssh`, `sshcraft-local`, `sshcraft-sync`, `sshcraft-fido`, `sshcraft-core`, `sshcraft-server`).

- [ ] **Step 1:** Grundlinie festhalten, auf `main` vor jeder Änderung: `cargo test --workspace 2>&1 | Select-String 'test result'`; die Summe der bestandenen Tests **ohne** `sshcraft-agent`, `sshcraft-cli` und `app/src-tauri` notieren. Sie ist die Messlatte für Task 2.
- [ ] **Step 2:** `git tag -a tauri-final 38ab865 -m "Last Tauri version"` und `git push origin tauri-final`.
- [ ] **Step 3:** Arbeitsbaum `git worktree add G:\Claude\worktrees\sshcraft-r1 -b r1-geruest main`.
- [ ] **Step 4:** `app/`, `crates/sshcraft-agent/`, `crates/sshcraft-cli/` löschen; Ordner umbenennen wie oben. Was R2 bis R6 davon noch brauchen (Tauri-Befehle, Prüfung der Agents, MCP-Code), holen sie aus dem Tag `tauri-final`.
- [ ] **Step 5:** `Cargo.toml` nach den Global Constraints neu schreiben: Glob-Mitglieder, `[workspace.package]`, `[workspace.lints]`, `[workspace.dependencies]` ohne `sshcraft-agent`, `windows`-Einträge nur noch dort, wo `fido` sie braucht, Pfade auf die neuen Ordner. Abhängigkeiten, die nur Tauri brauchte, fallen weg.
- [ ] **Step 6:** `LICENSE` löschen. `LICENSE-APACHE` mit dem Standardtext der Apache License 2.0 (wie in gridcraft), `LICENSE-MIT` mit der Zeile `Copyright (c) 2026 Musiker15 and the SSHCraft contributors`. `NOTICE`: Projektname, dieselbe Copyright-Zeile, "Licensed under the MIT License or the Apache License, Version 2.0, at your option (LICENSE-MIT, LICENSE-APACHE).", dazu "Versions up to the tag `tauri-final` were published under AGPL-3.0-only." und ein Abschnitt "Third-party material" mit "None bundled yet."
- [ ] **Step 7:** `.gitignore`: `node_modules/`, `/app/dist/`, `/app/src-tauri/gen/schemas/` raus; `/plan/` und `/target/` drin.
- [ ] **Step 8:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`: alles grün, Testzahl gleich der Grundlinie.
- [ ] **Step 9:** Commit `R1.1: tag tauri-final, drop the Tauri app, MIT OR Apache-2.0`.

### Task 2: `sshcraft-core` aufteilen, `platform`

**Files:** Create `crates/platform/{Cargo.toml, src/lib.rs}`, `crates/model/`, `crates/store/`, `crates/import/` (Module aus `crates/sshcraft-core/src/` samt ihren `*_tests.rs` per `git mv`); Move `crates/sshcraft-core/src/authorized_keys.rs` (+ Tests) nach `crates/ssh/src/`; Delete `crates/sshcraft-core/`; Modify `crates/{ssh,local,sync}/Cargo.toml` und ihre `use sshcraft_core::…`.

**Interfaces, Produces:**
- `sshcraft_platform::data_dir() -> Result<PathBuf, PlatformError>`: `directories::ProjectDirs::from("de", "musiker15", "SSHCraft")` und dessen `data_dir()`; `PlatformError::NoHome`, wenn `ProjectDirs` `None` liefert.
- `sshcraft_platform::home_dir() -> Option<PathBuf>` über `directories::BaseDirs`.
- `sshcraft-model`: Module `model`, `forward`, `keytypes`, `policy`, `logtext`, `os_detect`; Abhängigkeiten nur `serde`, `serde_json`, `uuid`, `thiserror` (kein `russh`, kein `rusqlite`, wasm-fähig).
- `sshcraft-store`: Module `store` (mit `credential_store`, `share_store`, `sync_store` als Untermodule wie bisher), `vault`, `session_log`, `agent`, `history`, `snippets`; Feature `testing` wie bisher in core.
- `sshcraft-import`: Module `import` (csv, keyfiles, knownhosts, mobaxterm, putty, securecrt), `sshconfig`, `export`.
- `sshcraft_ssh::authorized_keys` mit derselben öffentlichen Schnittstelle wie bisher.
- Kein Ersatz-Crate `sshcraft-core` und keine Re-Exporte: Aufrufer wechseln auf die neuen Pfade.

- [ ] **Step 1:** Tests zuerst, in `crates/platform/src/lib.rs`: `data_dir_matches_the_tauri_location` (Ergebnis endet unter Windows auf `musiker15\SSHCraft\data`; auf anderen Systemen gleich `ProjectDirs::from("de", "musiker15", "SSHCraft")` und dessen `data_dir()`). In `crates/store`: `keyring_service_is_unchanged` (`KEYRING_SERVICE == "de.musiker15.sshcraft"`, die Konstante wird dafür `pub(crate)`) und `schema_11_reopens_without_migration` (frische Datenbank öffnen, schließen, wieder öffnen: Schema 11, keine Migration gelaufen). Laufen lassen: rot, weil die Crates fehlen.
- [ ] **Step 2:** Crates anlegen, Module verschieben, Abhängigkeiten setzen: `ssh` hängt an `sshcraft-model`; `local` an `sshcraft-model`; `sync` an `sshcraft-model` und `sshcraft-store` (Dev-Abhängigkeit `sshcraft-store` mit Feature `testing`); `store` an `sshcraft-model`; `import` an `sshcraft-model` und `sshcraft-store`.
- [ ] **Step 3:** Tests grün; Gegenprobe für die drei neuen Tests (Ordnername `SSHCraft` zu `sshcraft`, Dienstname ändern, `SCHEMA_VERSION` auf 12): je rot, dann zurück.
- [ ] **Step 4:** fmt, clippy, `cargo test --workspace`: grün, Testzahl gleich Grundlinie plus 3.
- [ ] **Step 5:** Commit `R1.2: split sshcraft-core into model, store and import; add platform`.

### Task 3: Sync-Server als App

**Files:** Move `crates/sshcraft-server/` nach `crates/server/` (Paket `sshcraft-server-lib`), `crates/server/src/main.rs` nach `apps/sshcraft-server/src/main.rs` mit `apps/sshcraft-server/Cargo.toml` (Paket `sshcraft-server`, `[[bin]] name = "sshcraft-server"`); `crates/sshcraft-server/Dockerfile` nach `packaging/docker/Dockerfile`; Modify `.dockerignore`, `crates/sync/Cargo.toml` (Dev-Abhängigkeit auf `sshcraft-server-lib`), `docs/server.md`.

**Interfaces, Produces:** gleiche Befehle, Optionen und Vorgaben des Programms `sshcraft-server` wie bisher (`--database-url` mit Vorgabe `sqlite://sshcraft.db` usw.); die Tests unter `crates/server/tests/` laufen gegen die Bibliothek.

- [ ] **Step 1:** Test zuerst in `apps/sshcraft-server`: `cli_defaults_are_unchanged` (Vorgaben aus `clap` gleich den bisherigen, darunter `sqlite://sshcraft.db`). Rot.
- [ ] **Step 2:** Umzug; `main.rs` ruft nur noch die Bibliothek. Dockerfile-Pfade auf `apps/sshcraft-server` und den Workspace anpassen; `docker build -f packaging/docker/Dockerfile .` muss bauen, wenn Docker vorhanden ist, sonst steht im Commit, dass es nicht lief.
- [ ] **Step 3:** Tests grün, `cargo test -p sshcraft-server-lib --test store_contract` ohne `SSHCRAFT_TEST_POSTGRES_URL` überspringt die PostgreSQL-Fälle wie bisher.
- [ ] **Step 4:** fmt, clippy, `cargo test --workspace` grün.
- [ ] **Step 5:** Commit `R1.3: sync server as apps/sshcraft-server`.

### Task 4: Serielle Verbindung über serial2

**Files:** Modify `crates/local/Cargo.toml` (`serialport` raus, `serial2 = "0.2.39"` rein), `crates/local/src/serial.rs`.

**Interfaces, Consumes:** `SerialSettings`, `Parity`, `FlowControl`, `SerialError`, `SerialSettings::validate`, `list_ports() -> Vec<SerialPortInfo>`, `open(...)`, `SerialWriter::write` bleiben in Name und Form gleich.

- [ ] **Step 1:** Tests zuerst: `settings_map_to_serial2` als Tabelle über alle Werte (Datenbits 5 bis 8 auf `CharSize::Bits5` bis `Bits8`, Stoppbits 1 und 2, Parität None, Odd, Even, Fluss None, Software (XON/XOFF), Hardware (RTS/CTS), Baudrate unverändert); `invalid_data_bits_are_an_error` (4 und 9 liefern `SerialError`, statt still auf 8 zu fallen); `invalid_stop_bits_are_an_error` (0 und 3). Rot.
- [ ] **Step 2:** `serial.rs` auf serial2 umstellen; die Abbildung als eigene Funktion `fn to_serial2(settings: &SerialSettings) -> Result<serial2::Settings, SerialError>` (oder die Teilwerte, je nach serial2-API), die `validate` nutzt. `list_ports` liefert mit serial2 nur Pfade (`COM3`, `/dev/ttyUSB0`); `SerialPortInfo` behält seine Felder, die Beschreibung bleibt leer. Das steht im Commit und in `docs/connections.md`.
- [ ] **Step 3:** Tests grün; Gegenprobe: Parität Odd auf Even abbilden, `settings_map_to_serial2` wird rot; zurück.
- [ ] **Step 4:** fmt, clippy, `cargo test --workspace` grün. Danach steht keine eigene MPL-2.0-Abhängigkeit mehr im Baum.
- [ ] **Step 5:** Commit `R1.4: serial connections through serial2`.

### Task 5: `xtask` und Werkzeug-Konfiguration

**Files:** Create `xtask/{Cargo.toml, src/main.rs, src/layers.rs, src/assets.rs}`, `.cargo/config.toml` (`[alias] xtask = "run --package xtask --"`), `.config/nextest.toml` (`nextest-version = "0.9.152"`), `clippy.toml`, `rustfmt.toml`, `deny.toml`.

**Interfaces, Produces:**
- `cargo xtask ci | layers | assets | deny | wasm | test`; `ci` führt nacheinander aus und meldet am Ende jede Stufe mit ok oder FAIL: `fmt` (`cargo fmt --all -- --check`), `clippy` (`cargo clippy --workspace --all-targets --keep-going -- -D warnings`), `test` (`cargo nextest run --workspace` wenn vorhanden, sonst `cargo test --workspace`, danach `cargo test --workspace --doc`), `layers`, `assets`, `deny`, `wasm`.
- `layers::Class { Layer(u8), Exempt }`; `layers::TABLE: &[(&str, Class)]` = proto 0, platform 0, model 1, store 2, import 3, ssh 3, local 3, sync 3, server-lib 3, fido 4; `xtask` und `sshcraft-server` (App) `Exempt`. Später kommen engine 5, ui-egui 6, mcp 6, term 2.
- `layers::Crate { name: String, deps: Vec<String> }` (Name ohne `sshcraft-`, nur normale und Build-Abhängigkeiten; Dev-Abhängigkeiten zählen nicht), `layers::check(&[Crate]) -> Vec<Violation>`, `Violation { from: String, to: String, why: &'static str }`. Regeln: Abhängigkeit nur auf eine **tiefere** Schicht; ein Workspace-Crate ohne Tabelleneintrag ist ein Verstoß; unter L6 keine Abhängigkeit auf `egui`, `eframe`, `winit`, `rfd`. `layers` baut die Liste aus `cargo metadata --format-version 1`.
- `assets::check(files: &[PathBuf], attribution: &str) -> Vec<String>`: jede von `git ls-files` gelistete Datei mit Endung `png svg ico icns jpg jpeg gif webp bmp tif tiff wav mp3 ogg` braucht eine Tabellenzeile in `ATTRIBUTION.md`, deren erste Zelle den Pfad in Backticks enthält; jede Datei mit Endung `ttf otf woff woff2` ist immer ein Fehler (Schriften gehören nach craft-fonts).
- `deny` ruft `cargo deny check licenses bans sources advisories`.
- `wasm` ruft `cargo check --target wasm32-unknown-unknown -p sshcraft-proto -p sshcraft-model`.

- [ ] **Step 1:** Tests zuerst in `layers.rs`: `upward_dependency_is_a_violation` (model nach store), `same_layer_dependency_is_a_violation` (ssh nach sync), `unregistered_crate_is_a_violation` (`foo`), `dev_dependencies_are_ignored`, `ui_toolkit_below_l6_is_a_violation` (store nach `egui`), `current_table_without_toolkit_has_no_violation`. In `assets.rs`: `unlisted_asset_fails`, `listed_asset_passes`, `font_file_always_fails`. Rot.
- [ ] **Step 2:** `layers.rs`, `assets.rs`, `main.rs` schreiben (Aufbau wie gridcraft `xtask/src/main.rs`: `cargo()`-Helfer, `run(cmd, what)`, Stufenliste für `ci`).
- [ ] **Step 3:** Tests grün; Gegenprobe: in `check` `<` gegen `<=` tauschen, `same_layer_dependency_is_a_violation` wird rot; Schriftregel entfernen, `font_file_always_fails` wird rot. Zurück.
- [ ] **Step 4:** `clippy.toml`: `allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests` jeweils `true`, `cognitive-complexity-threshold = 30`. `rustfmt.toml`: `max_width = 150`, `use_small_heuristics = "Max"`; danach einmal `cargo fmt --all` (eigener Commit-Schritt unten, damit der Formatierungsumbau getrennt bleibt).
- [ ] **Step 5:** `deny.toml`: `[licenses] allow` = genau die Liste der House rules (`ISC`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`, `Apache-2.0`, `OFL-1.1`, `Ubuntu-font-1.0`, `Bitstream-Vera`, `CC0-1.0`, `CC-BY-4.0`, `CC-BY-SA-4.0`, `LicenseRef-Public-Domain`), `confidence-threshold = 0.9`; `[bans] wildcards = "deny"`; `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`; `[advisories] yanked = "warn"`. `cargo deny check` laufen lassen; jede Lizenz, die nicht passt, bekommt `[[licenses.exceptions]]` für genau dieses Crate mit Kommentar, warum es im Baum ist (Muster: pdfcraft `deny.toml`). Copyleft außer der in Spec Abschnitt 14 genannten MPL-2.0 von `option-ext`, und jede Sicherheitsmeldung ohne behobene Fassung: anhalten und Moritz fragen.
- [ ] **Step 6:** wasm: für `proto` und `model` die Zufallsquelle im Browser einrichten, wie craft-libs es macht: `[target.'cfg(target_arch = "wasm32")'.dependencies] getrandom = { version = <neueste der verwendeten Reihe>, features = ["wasm_js"] }`, bei `uuid` das passende wasm-Feature, und in `.cargo/config.toml` `[target.wasm32-unknown-unknown] rustflags = ['--cfg', 'getrandom_backend="wasm_js"']`, falls die verwendete getrandom-Reihe es verlangt. `cargo xtask wasm` grün.
- [ ] **Step 7:** `cargo xtask ci` grün.
- [ ] **Step 8:** Zwei Commits: `R1.5: rustfmt settings of the Craft apps` (nur die Formatierung) und `R1.5: xtask ci, layers, assets, deny and wasm`.

### Task 6: Nie abstürzen

**Files:** Modify alle Crate-Wurzeln (`lib.rs`), `crates/ssh/src/tunnel.rs`, `crates/local/src/pty.rs`, `crates/local/src/serial.rs:171`, `crates/proto/src/identity.rs`, `crates/proto/src/record.rs`, `crates/sync/src/{account.rs, areas.rs, shares.rs}`, `crates/server/src/state.rs`, `apps/sshcraft-server/src/main.rs`, `crates/fido/src/soft.rs`, die `testing`-Module; Create `crates/fido/src/presence.rs` (aus `app/src-tauri/src/hello.rs` im Tag `tauri-final`); Modify `crates/fido/Cargo.toml`.

**Interfaces, Produces:** `sshcraft_fido::presence::{Presence, UserPresence, WindowsHello, CancelOnDrop, Cancellable}` mit derselben Schnittstelle wie in `hello.rs` (R2 nutzt sie in der Engine).

Stand heute im Nicht-Test-Code: proto 4 (identity 3, record 1), ssh 6 (lib 1, tunnel 5), local 8 (pty 7, serial 1), sync 5 (account 1, areas 1, shares 3), server 2 (main 1, state 1), fido 4 (soft 4). Die meisten sind `lock().expect(…)`.

- [ ] **Step 1:** Tests zuerst: `tunnel::tests::poisoned_remotes_lock_does_not_panic` (ein Thread hält die Sperre und stirbt; danach Weiterleitung anlegen und entfernen ohne Panic), `pty::tests::poisoned_writer_lock_does_not_panic` und `serial::tests::poisoned_port_lock_does_not_panic` nach demselben Muster. Für jede übrige Stelle, die Eingaben erreichen (Server-Antwort, Datei, Parameter), ein Test mit genau der Eingabe, die heute panikt. Rot.
- [ ] **Step 2:** Sperren mit `lock().unwrap_or_else(PoisonError::into_inner)`. Übrige Stellen: von Eingaben erreichbar heißt `Result` über den Fehlertyp des Crates; nachweislich unmöglich heißt `#[allow(clippy::expect_used)]` an der Stelle und `.expect("<warum es nicht scheitern kann>")`.
- [ ] **Step 3:** Jede Produktions-Crate-Wurzel bekommt das `#![deny(...)]`-Attribut aus den Global Constraints. Module, die nur mit dem Feature `testing` gebaut werden, beginnen mit `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` und einem Kommentar, dass es Testhilfen sind.
- [ ] **Step 4:** `fido` als die eine `unsafe`-Ausnahme, nach dem Muster von `photocraft/crates/tablet/Cargo.toml`: statt `[lints] workspace = true` eigene `[lints.rust] unsafe_code = "deny"` und die clippy-Einträge des Workspace wörtlich, mit Kommentar, warum. `webauthn.rs` und `presence.rs` beginnen mit `#![allow(unsafe_code)]`; jeder `unsafe`-Block hat einen `// SAFETY:`-Kommentar. `presence.rs` aus `git show tauri-final:app/src-tauri/src/hello.rs`, seine Tests mit.
- [ ] **Step 5:** Tests grün; Gegenprobe: eine Sperre in `tunnel.rs` zurück auf `.expect`, Test wird rot; zurück.
- [ ] **Step 6:** `cargo xtask ci` grün.
- [ ] **Step 7:** Commit `R1.6: no panics outside tests, fido as the one unsafe exception`.

### Task 7: Regeln, Doku und CI

**Files:** Create `AGENTS.md`, `CLAUDE.md`, `SECURITY.md`, `ROADMAP.md`, `GLOSSARY.md`, `ATTRIBUTION.md`, `contributors/people.toml`, `contributors/contributors.json`, `docs/architecture.md`, `docs/decisions.md`, `docs/gaps.md`, `docs/roadmap.md`, `docs/target-app-parity.md`, `.coderabbit.yaml`, `.github/workflows/house-rules.yml`, `.github/workflows/server-postgres.yml`, `.github/dependabot.yml`; Modify `README.md`, `docs/security.md`, `.github/workflows/ci.yml`, die übrigen `docs/*.md` (Hinweiszeile).

- [ ] **Step 1: House rules.** `storytold/craft-contrib` flach in einen leeren Ordner im Scratchpad klonen, Commit `S` = Kopf von `main`. Für `core` (`standards/house-rules-core.md`) und `rust` (`standards/house-rules-rust.md`) je einen Block nach dem Verfahren von `craft-common-actions/actions/house-rules/house_rules.py` erzeugen: Text als UTF-8 lesen, BOM weg, `\r\n` und `\r` zu `\n`, abschließende Zeilenumbrüche weg und genau ein `\n` an; `sha256` über diese Bytes; Block = `<!-- craft-house-rules:start <name> source=<S> sha256=<hex> -->\n` + Text + `<!-- craft-house-rules:end -->\n`. Das Skript selbst wird nicht ausgeführt; die Prüfung macht die CI (`house-rules.yml`).
- [ ] **Step 2: `AGENTS.md`** = Block `core`, Block `rust`, dann `# SSHCraft: instructions for agents` mit den Abschnitten aus Spec Abschnitt 11: Start every session here (`ROADMAP.md`, `docs/gaps.md`; `plan/` optional), Assets (nichts von Termius oder anderen Herstellern, Funktionsnamen ja; jede Asset-Datei in `ATTRIBUTION.md`; Schriften nur aus craft-fonts), Never crash (Regeln, `panic = "unwind"`, die `fido`-Ausnahme mit Grund), Non-negotiables (Clean-Room gegenüber Termius; Layering mit der Tabelle aus Task 5; Rust only; wasm nur für `proto` und `model`, mit Grund; Quality gates `cargo xtask ci`; "Everything is a command" als kommende Regel ab R2), Security rules (die 13 Regeln aus Spec Abschnitt 17 und die Agent-Regeln aus Abschnitt 6.2 und 9 in Kurzform), Running (`cargo run -p sshcraft-server -- --help`), Roadmap, Contributor credits (wie gridcraft), CI workflows (geteilte Workflows; `server-postgres.yml` als begründete Ausnahme). Keine Pfade dieser Maschine.
- [ ] **Step 3:** `CLAUDE.md` enthält genau `@AGENTS.md`. `README.md` englisch und klein nach Moritz' README-Regel (Titel, ein Satz, ein bis zwei Absätze, Stand "rebuild in progress, see ROADMAP.md", Schnellstart mit einem Codeblock für den Server, Tabelle "Documentation", Help and contributing, License als letzter Abschnitt). `SECURITY.md` nach dem Muster von wordcraft mit dem Umfang von SSHCraft. `ROADMAP.md` eine Seite aus dem Fahrplan (R1 bis F7). `ATTRIBUTION.md` mit Richtlinie und leerer Tabelle (Kopfzeile bleibt). `GLOSSARY.md` mit den Begriffen aus Spec 2026-11-11 Abschnitt 4. `contributors/people.toml` nur mit der Vorlage im Kopf (kein Eintrag ohne Moritz' Zustimmung, House rule 6), `contributors/contributors.json` = `[]`.
- [ ] **Step 4:** `docs/architecture.md` (Schichtentabelle, Datenfluss nach R1, Verweis auf den Neuaufbau-Entwurf), `docs/decisions.md` (die tragenden Entscheidungen aus Spec 2026-11-11 und 2026-11-14, englisch, je eine Zeile mit Datum), `docs/gaps.md` und `docs/roadmap.md` (R2 bis F7 als offene Arbeit), `docs/target-app-parity.md` (Gerüst: Bereiche aus dem Termius-Funktionsumfang mit Stand "Tauri version only" bis R5). `docs/security.md`: der CSP-Abschnitt der Tauri-Fassung fällt weg, die Regeln aus Spec Abschnitt 17 kommen dazu. Jede übrige Feature-Doku bekommt oben die Zeile "The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps."
- [ ] **Step 5: CI.** `ci.yml` ersetzen: Auslöser `push` auf `main`, `pull_request`, `workflow_dispatch`; `permissions: contents: read`; `concurrency` wie gridcraft; ein Job `gate` mit `uses: storytold/craft-common-actions/.github/workflows/gate.yml@main` und `os: '["windows-latest"]'` (Linux und macOS kommen in R7), `craft-fonts-ref: ""`, `tools: cargo-nextest@0.9.152,cargo-deny@0.20.2`, `targets: wasm32-unknown-unknown`. `house-rules.yml` ruft `house-rules.yml@main` mit `rust: true`. `server-postgres.yml` übernimmt den bisherigen PostgreSQL-Job (Test jetzt `-p sshcraft-server-lib`) mit dem Kommentar, dass kein geteilter Workflow einen PostgreSQL-Dienst anbietet und der Job umzieht, sobald es einen gibt (House rule 11). `dependabot.yml` für `cargo` und `github-actions`, wöchentlich. `.coderabbit.yaml` wie gridcraft.
- [ ] **Step 6: Prüfungen.** `git grep -n -i "agpl"` findet nur `NOTICE` (der Satz zur Historie) und Dateien unter `docs/superpowers/`. `git grep -n -i "tauri"` findet nur `NOTICE`, `docs/superpowers/`, `docs/decisions.md` und die Hinweiszeilen. Alle neuen Textdateien gültiges UTF-8 ohne BOM, keine Gedankenstriche (U+2014) in `README.md`, `AGENTS.md`, `SECURITY.md`.
- [ ] **Step 7:** `cargo xtask ci` grün.
- [ ] **Step 8:** Commit `R1.7: AGENTS.md with the house rules, docs and CI of the Craft apps`.
- [ ] **Step 9:** Push, Pull Request gegen `main` (englische Beschreibung ohne KI-Hinweis), CI abwarten: `gate`, `house-rules` und `server-postgres` grün. Ein roter `house-rules`-Lauf heißt: Block oder Prüfsumme falsch, nicht die Prüfung anpassen.
- [ ] **Step 10:** Abschluss-Review des ganzen Zweigs auf opus, Befunde mit Test und Gegenprobe beheben, dann Merge über den Pull Request.
