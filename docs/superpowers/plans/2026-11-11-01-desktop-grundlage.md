# SSHCraft Teilplan 1: Desktop-Grundlage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eine Windows-App, in der man Gruppen und Hosts anlegt, eigene SSH-Schlüssel importiert oder erzeugt (nie im Klartext auf der Platte) und sich in Terminal-Tabs mit Servern verbindet, mit Host-Fingerprint-Prüfung, Keep-Alive und automatischem Wiederverbinden.

**Architecture:** Cargo-Workspace mit zwei Bibliotheken und einer Tauri-App. `sshcraft-core` hält Datenmodell, SQLite-Speicher, den verschlüsselten Schlüsseltresor und reine Entscheidungslogik (Fingerprint-Prüfung, Wiederverbinden). `sshcraft-ssh` kapselt `russh` hinter einer kleinen Sitzungs-Schnittstelle. Die Tauri-App verbindet beides mit einer React-Oberfläche; private Schlüssel verlassen den Rust-Teil nie.

**Tech Stack:** Rust 1.99 (Edition 2024), Tauri 2.12.5 (stabil; 3.0 ist Alpha und bewusst nicht gewählt), russh 0.65.1, rusqlite 0.42.0 (Feature `bundled`), keyring 4.2.0, chacha20poly1305 0.11.0, tokio 1.54.1, uuid 1.29.2, serde 1.0.229, serde_json 1.0.154, thiserror 2.0.21, zeroize 1.9.1, rand 0.10.3, directories 6.0.0, async-trait 0.1.92, tempfile 3.27.0 (Tests); React 19.3.1, Vite 8.3.8, @vitejs/plugin-react 6.3.0, TypeScript 7.0.2, @tauri-apps/api und @tauri-apps/cli 2.12.4, @xterm/xterm 6.0.0, @xterm/addon-fit 0.11.0, Vitest 5.1.0, jsdom 31.0.0.

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`, Fahrplan: `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md`

## Global Constraints

- Arbeitsbäume nur unter `G:\Claude\worktrees`, nie auf Laufwerk C.
- Code, Kommentare, Bezeichner, Testnamen, Dateinamen von Code-Dateien: Englisch. README: Englisch (internationales Publikum). Commit-Nachrichten: Deutsch.
- Commits **ohne** `Co-Authored-By`-Trailer und ohne jede KI-Zuschreibung. Keine Gedankenstriche (—) in Commit-Nachrichten, README oder PR-Texten.
- Alle Textdateien UTF-8.
- Lizenz AGPL-3.0, Repository `musiker15/sshcraft`.
- Vor dem Eintragen einer Abhängigkeit die neueste stabile Version prüfen (`cargo search`, `npm view`); die Versionen oben sind der Stand vom 11.11.2026.
- Ein privater Schlüssel liegt nie unverschlüsselt auf der Platte, erscheint nie in einem Log und wird von keinem Tauri-Befehl an die Oberfläche zurückgegeben.
- SSH-Anmeldung nur mit Schlüsseln, kein Passwort (Fahrplan, letzter Absatz).
- Ein abweichender Host-Fingerprint bricht die Verbindung ab; es gibt keinen Weg, ihn im Verbindungsdialog zu übergehen.
- Lokal vor dem Push nur das betroffene Paket prüfen (`cargo fmt --check`, `cargo clippy -p <crate> -- -D warnings`, `cargo test -p <crate>` bzw. `npm run typecheck && npm test` in `app/`); den Rest prüft die CI.
- Falls `russh` mit dem Standard-Feature `aws-lc-rs` unter Windows nicht baut: `default-features = false, features = ["ring", "flate2", "rsa"]` und das im Commit begründen.

## Review Focus

1. **Server-Schlüssel hat sich geändert** (Neuinstallation oder Angriff): Verbindung bricht mit deutlicher Warnung ab, auch beim automatischen Wiederverbinden; nur "Fingerprint vergessen" in den Host-Einstellungen macht den Weg frei. Tests in Task 4 und Task 6.
2. **Eingefügter Schlüssel mit Windows-Zeilenenden, Leerzeilen am Ende oder Passphrase:** Import gelingt; falsche Passphrase und Datenmüll ergeben eine verständliche Fehlermeldung, kein Absturz. Tests in Task 3.
3. **Verbindung reißt ab** (WLAN-Wechsel, Ruhezustand): Status "Verbindung verloren, neuer Versuch in N s", bis zu fünf Versuche; ein normales `exit` verbindet nicht neu. Tests in Task 4 und Task 6.
4. **Viel Ausgabe und Unicode** (`cat` einer großen Datei, Umlaute, Emojis, UTF-8-Zeichen über Paketgrenzen): alles kommt vollständig und in Reihenfolge an, die Oberfläche bleibt bedienbar. Tests in Task 5 und Task 7.
5. **Schlüsselspeicher des Systems liefert den Hauptschlüssel nicht** (neues Windows-Profil, gelöschter Eintrag in der Anmeldeinformationsverwaltung), obwohl verschlüsselte Schlüssel existieren: klare Fehlermeldung, auf keinen Fall ein neuer Hauptschlüssel, der die vorhandenen Schlüssel unlesbar macht. Test in Task 3.

---

## Dateistruktur

```
sshcraft/
├── Cargo.toml                      Workspace: crates/*, app/src-tauri
├── LICENSE                         AGPL-3.0 Volltext
├── README.md                       kurz, Englisch
├── .gitignore, .protonignore       target/, node_modules/, app/dist/
├── .github/workflows/ci.yml
├── crates/
│   ├── sshcraft-core/src/
│   │   ├── lib.rs
│   │   ├── model.rs                Ids, Group, Host, KeyRef, resolve()
│   │   ├── store.rs                SQLite, Schema, CRUD, known hosts
│   │   ├── vault.rs                Hauptschlüssel, Verschlüsselung privater Schlüssel
│   │   └── policy.rs               check_host_key(), next_reconnect_delay()
│   └── sshcraft-ssh/
│       ├── src/lib.rs              connect(), Session, Shell, ShellEvent, SshError
│       ├── src/testing.rs          In-Process-Testserver mit russh::server (Feature "testing")
│       └── tests/session.rs
└── app/
    ├── package.json, vite.config.ts, tsconfig.json, index.html
    ├── src/
    │   ├── main.tsx, App.tsx
    │   ├── api.ts                  typisierte invoke-Wrapper
    │   ├── state/tabs.ts (+ .test.ts)
    │   ├── forms/hostForm.ts (+ .test.ts)
    │   └── components/ Sidebar, GroupForm, HostForm, KeyManager, TerminalTabs, TerminalView, HostKeyDialog
    └── src-tauri/src/
        ├── lib.rs                  Setup, AppState, Befehle registrieren
        ├── state.rs                AppState { store: Mutex<Store>, vault: Vault, sessions: SessionManager }
        ├── commands/data.rs        Gruppen, Hosts, known hosts
        ├── commands/keys.rs        Schlüssel
        ├── commands/sessions.rs    Sitzungen
        └── sessions.rs             SessionManager, Wiederverbinden, Host-Key-Rückfrage
```

---

### Task 1: Repository-Gerüst und CI

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `README.md`, `.gitignore`, `.protonignore`, `.github/workflows/ci.yml`, `crates/sshcraft-core/{Cargo.toml,src/lib.rs}`, `crates/sshcraft-ssh/{Cargo.toml,src/lib.rs}`
- Create: `app/` über `npm create tauri-app@latest` (Vorlage React, TypeScript, npm), danach Versionen auf die oben genannten anheben

**Interfaces:**
- Produces: Workspace-Mitglieder `sshcraft-core`, `sshcraft-ssh`, `sshcraft-app` (`app/src-tauri`); npm-Skripte in `app/package.json`: `dev`, `build`, `typecheck` (`tsc --noEmit`), `test` (`vitest run`), `tauri`.

- [ ] **Step 1:** `git init` in `G:\GitHub Repositories\Musiker15\sshcraft`, Standardzweig `main`. Die vorhandenen `docs/` bleiben.
- [ ] **Step 2:** Workspace-`Cargo.toml` mit `resolver = "3"`, `[workspace.package] edition = "2024", license = "AGPL-3.0-only"`, gemeinsame Abhängigkeiten unter `[workspace.dependencies]` mit den Versionen aus dem Kopf.
- [ ] **Step 3:** Tauri-App unter `app/` erzeugen, `app/src-tauri/Cargo.toml` in den Workspace aufnehmen, Bezeichner `de.musiker15.sshcraft`, Fenstertitel "SSHCraft".
- [ ] **Step 4:** `README.md`: Titel, ein Satz ("Self-hosted SSH client with end-to-end encrypted sync and native Proton Pass support."), Stand "early development", Schnellstart (`cd app && npm install && npm run tauri dev`), Tabelle "Documentation" mit Link auf das Design, Lizenz als letzter Abschnitt.
- [ ] **Step 5:** `ci.yml` auf `windows-latest`, ausgelöst bei Push und Pull Request: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, in `app/`: `npm ci`, `npm run typecheck`, `npm test`. Job-Kennungen Englisch, `name:`-Felder Deutsch.
- [ ] **Step 6:** Prüfen: `cargo build --workspace` und `cd app && npm run typecheck && npm test` laufen durch (Vitest ohne Tests mit `--passWithNoTests`).
- [ ] **Step 7:** Commit `Projektgerüst mit Tauri-App, Rust-Workspace und CI`.

---

### Task 2: Datenmodell und lokaler Speicher

**Files:**
- Create: `crates/sshcraft-core/src/model.rs`, `crates/sshcraft-core/src/store.rs`
- Modify: `crates/sshcraft-core/src/lib.rs`

**Interfaces:**
- Produces (`sshcraft_core::model`):
  - `GroupId(Uuid)`, `HostId(Uuid)`, `KeyId(Uuid)`: `Copy`, `Serialize`/`Deserialize` als UUID-String, je `fn new() -> Self` (v4).
  - `enum KeyRef { Stored(KeyId) }` (Teilplan 3 ergänzt `Agent { fingerprint: String }`), serde-Tag `"kind"`.
  - `struct ConnectionSettings { address: Option<String>, port: Option<u16>, username: Option<String>, key: Option<KeyRef> }`, `Default`.
  - `struct Group { id: GroupId, name: String, defaults: ConnectionSettings }`
  - `struct Host { id: HostId, name: String, group_id: Option<GroupId>, settings: ConnectionSettings }`
  - `struct ResolvedHost { host_id: HostId, name: String, address: String, port: u16, username: String, key: KeyRef }`
  - `fn resolve(host: &Host, group: Option<&Group>) -> Result<ResolvedHost, ResolveError>`
  - `enum ResolveError { MissingAddress, MissingUsername, MissingKey, WrongGroup }` (`thiserror`)
- Produces (`sshcraft_core::store`):
  - `Store::open(path: &Path) -> Result<Store, StoreError>`, `Store::open_in_memory() -> Result<Store, StoreError>`
  - `upsert_group(&self, &Group)`, `delete_group(&self, GroupId)` (löscht die Hosts der Gruppe mit), `list_groups(&self) -> Vec<Group>`
  - `upsert_host(&self, &Host)`, `delete_host(&self, HostId)`, `list_hosts(&self) -> Vec<Host>`
  - `known_host(&self, address: &str, port: u16) -> Option<String>`, `set_known_host(&self, address: &str, port: u16, fingerprint: &str)`, `forget_known_host(&self, address: &str, port: u16)`
  - für den Tresor (Task 3), `pub` aber nur dort benutzt: `put_key_record(&self, &KeyRecord)`, `key_record(&self, KeyId) -> Option<KeyRecord>`, `list_key_records(&self) -> Vec<KeyRecord>`, `delete_key_record(&self, KeyId)`, `has_key_records(&self) -> bool`
  - `struct KeyRecord { id: KeyId, name: String, algorithm: String, fingerprint: String, public_key: String, sealed_secret: Vec<u8> }`
  - Alle Methoden geben `Result<_, StoreError>` zurück.

- [ ] **Step 1: Failing tests für `resolve`** in `model.rs`:

```rust
#[test] fn host_inherits_group_defaults() {
    // group: address "example.org", port 2222, username "root", key Stored(k)
    // host: settings all None, group_id = group.id
    // resolve -> address "example.org", port 2222, username "root", key Stored(k)
}
#[test] fn host_overrides_group() { /* host.username = Some("deploy") wins over "root" */ }
#[test] fn port_defaults_to_22() { /* neither host nor group sets a port -> 22 */ }
#[test] fn missing_address_is_error() { /* -> Err(ResolveError::MissingAddress) */ }
#[test] fn missing_key_is_error() { /* -> Err(ResolveError::MissingKey) */ }
#[test] fn wrong_group_is_error() { /* host.group_id != group.id -> Err(WrongGroup) */ }
```

- [ ] **Step 2:** `cargo test -p sshcraft-core` schlägt fehl (nicht kompilierbar).
- [ ] **Step 3:** Modell und `resolve` umsetzen. Leere Strings gelten wie `None` (nach `trim`).
- [ ] **Step 4: Failing tests für `Store`** (in `store.rs`, mit `open_in_memory`, ein Test mit `tempfile` und `open`):

```rust
#[test] fn group_and_host_roundtrip() { /* upsert, list returns equal values */ }
#[test] fn upsert_updates_existing() { /* same id, new name -> list has one entry with new name */ }
#[test] fn delete_group_removes_its_hosts() { /* host in group gone, host without group stays */ }
#[test] fn known_host_set_get_forget() { /* None -> Some("SHA256:abc") -> None */ }
#[test] fn data_survives_reopen() { /* tempfile path: write, drop, open again, read */ }
```

- [ ] **Step 5:** Tests schlagen fehl.
- [ ] **Step 6:** `Store` umsetzen: `rusqlite` mit `bundled`, Schema über `PRAGMA user_version` (Version 1), `ConnectionSettings` als JSON-Spalte, Fremdschlüssel an (`PRAGMA foreign_keys = ON`), `ON DELETE CASCADE` für Hosts einer Gruppe, Tabelle `known_hosts(address, port, fingerprint, PRIMARY KEY(address, port))`, Tabelle `keys` mit `sealed_secret BLOB`.
- [ ] **Step 7:** `cargo test -p sshcraft-core` grün.
- [ ] **Step 8:** Commit `Datenmodell mit Vererbung aus Gruppen und SQLite-Speicher`.

---

### Task 3: Schlüsseltresor

**Files:**
- Create: `crates/sshcraft-core/src/vault.rs`
- Modify: `crates/sshcraft-core/src/lib.rs`, `crates/sshcraft-core/Cargo.toml` (russh für `russh::keys`, chacha20poly1305, keyring, zeroize, rand)

**Interfaces:**
- Consumes: `Store`, `KeyRecord`, `KeyId` aus Task 2.
- Produces (`sshcraft_core::vault`):
  - `trait MasterKeyProvider: Send + Sync { fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError>; fn save(&self, key: &[u8; 32]) -> Result<(), VaultError>; }`
  - `struct KeyringMasterKey` (Dienst `"de.musiker15.sshcraft"`, Konto `"vault-master-key"`, Wert Base64), `struct MemoryMasterKey` (für Tests, `Default`).
  - `struct KeyInfo { id: KeyId, name: String, algorithm: String, fingerprint: String, public_key: String }` mit `Serialize`; **kein** Feld mit geheimem Inhalt.
  - `Vault::unlock(store: &Store, provider: Box<dyn MasterKeyProvider>) -> Result<Vault, VaultError>`
  - `Vault::import_key(&self, store: &Store, name: &str, openssh_private_key: &str, passphrase: Option<&str>) -> Result<KeyInfo, VaultError>`
  - `Vault::generate_ed25519(&self, store: &Store, name: &str) -> Result<KeyInfo, VaultError>`
  - `Vault::private_key(&self, store: &Store, id: KeyId) -> Result<russh::keys::PrivateKey, VaultError>`
  - freie Funktionen `list_keys(store: &Store) -> Result<Vec<KeyInfo>, VaultError>`, `delete_key(store: &Store, id: KeyId) -> Result<(), VaultError>`
  - `enum VaultError { MasterKeyMissing, InvalidKey(String), WrongPassphrase, Corrupt, NotFound, Keyring(String), Store(StoreError) }`
- Format von `sealed_secret`: Byte `1` (Formatversion) ‖ 24 Byte Nonce ‖ XChaCha20-Poly1305-Chiffrat des OpenSSH-Formats ohne Passphrase; Zusatzdaten (AAD) = die 16 Bytes der `KeyId`.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn import_then_load_returns_same_key() { /* fingerprint of loaded key == KeyInfo.fingerprint */ }
#[test] fn generate_ed25519_has_sha256_fingerprint() { /* KeyInfo.fingerprint starts with "SHA256:", algorithm "ssh-ed25519" */ }
#[test] fn import_accepts_crlf_and_trailing_whitespace() { /* key text with "\r\n" and "\n\n  " appended imports fine */ }
#[test] fn import_with_passphrase() { /* encrypted OpenSSH key + right passphrase -> Ok */ }
#[test] fn import_wrong_passphrase_is_error() { /* -> Err(VaultError::WrongPassphrase) */ }
#[test] fn import_garbage_is_error() { /* "hello" -> Err(VaultError::InvalidKey(_)) */ }
#[test] fn database_file_never_contains_plaintext_key() {
    // tempfile store, import a key, drop store, read file bytes:
    // assert!(!contains(b"OPENSSH PRIVATE KEY"));
    // assert!(!contains(&ed25519_private_seed_bytes));
}
#[test] fn sealed_secret_is_bound_to_key_id() {
    // import two keys, swap their sealed_secret in the DB, private_key(id) -> Err(VaultError::Corrupt)
}
#[test] fn unlock_refuses_new_master_key_when_secrets_exist() {
    // store with a key sealed under provider A; unlock with empty MemoryMasterKey -> Err(MasterKeyMissing)
    // and the empty provider must still be empty afterwards (no save happened)
}
#[test] fn key_info_json_has_no_secret() {
    // serde_json::to_string(&info) contains neither "PRIVATE" nor the base64 of the private seed
}
```

Testschlüssel mit Passphrase werden im Test über `russh::keys` erzeugt und verschlüsselt, keine Schlüsseldateien im Repository.

- [ ] **Step 2:** `cargo test -p sshcraft-core vault` schlägt fehl.
- [ ] **Step 3:** Tresor umsetzen. `unlock`: Hauptschlüssel laden; fehlt er und `store.has_key_records()` ist `false`, neuen erzeugen (`rand`) und speichern; fehlt er und es gibt Datensätze, `MasterKeyMissing`. Eingaben vor dem Parsen `trim`men und `\r\n` zu `\n` machen. Entschlüsselte Zwischenpuffer in `Zeroizing`.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenproben** (Regel 4: Datenweg und Rechte). Einzeln, jeweils danach zurücknehmen:
  - AAD weglassen: `sealed_secret_is_bound_to_key_id` muss rot werden.
  - Statt Chiffrat das Klartext-OpenSSH speichern: `database_file_never_contains_plaintext_key` muss rot werden.
  - In `unlock` immer einen neuen Hauptschlüssel erzeugen: `unlock_refuses_new_master_key_when_secrets_exist` muss rot werden.
  - `KeyInfo` um ein Feld `secret` ergänzen: `key_info_json_has_no_secret` muss rot werden.
- [ ] **Step 6:** Commit `Verschlüsselter Schlüsseltresor mit Hauptschlüssel im Systemspeicher`.

---

### Task 4: Entscheidungslogik für Host-Fingerprint und Wiederverbinden

**Files:**
- Create: `crates/sshcraft-core/src/policy.rs`
- Modify: `crates/sshcraft-core/src/lib.rs`

**Interfaces:**
- Produces (`sshcraft_core::policy`):
  - `enum HostKeyStatus { Trusted, Unknown, Mismatch { expected: String } }`
  - `fn check_host_key(known: Option<&str>, presented: &str) -> HostKeyStatus`
  - `enum Disconnect { Exited(Option<u32>), Lost(String), HostKeyRejected, AuthFailed }`
  - `fn next_reconnect_delay(reason: &Disconnect, attempt: u32) -> Option<Duration>`: Versuch 1 bis 5 ergibt 1 s, 2 s, 5 s, 10 s, 10 s, danach `None`; nur `Lost` führt zu einem Versuch.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn unknown_when_nothing_stored() { assert_eq!(check_host_key(None, "SHA256:a"), HostKeyStatus::Unknown) }
#[test] fn trusted_when_equal() { assert_eq!(check_host_key(Some("SHA256:a"), "SHA256:a"), HostKeyStatus::Trusted) }
#[test] fn mismatch_when_different() { /* Some("SHA256:a"), "SHA256:b" -> Mismatch { expected: "SHA256:a" } */ }
#[test] fn reconnect_delays_for_lost_connection() { /* attempts 1..=5 -> 1,2,5,10,10 s; 6 -> None */ }
#[test] fn no_reconnect_after_exit() { /* Exited(Some(0)), attempt 1 -> None */ }
#[test] fn no_reconnect_after_host_key_rejected() { /* HostKeyRejected -> None */ }
#[test] fn no_reconnect_after_auth_failed() { /* AuthFailed -> None */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenproben:** `check_host_key` bei Abweichung `Trusted` liefern lassen: `mismatch_when_different` muss rot werden. `HostKeyRejected` wie `Lost` behandeln: `no_reconnect_after_host_key_rejected` muss rot werden. Zurücknehmen.
- [ ] **Step 6:** Commit `Regeln für Host-Fingerprint und Wiederverbinden`.

---

### Task 5: SSH-Sitzungen

**Files:**
- Create: `crates/sshcraft-ssh/src/lib.rs`, `crates/sshcraft-ssh/src/testing.rs` (nur mit Feature `testing`), `crates/sshcraft-ssh/tests/session.rs`
- Modify: `crates/sshcraft-ssh/Cargo.toml` (sshcraft-core, russh, tokio, async-trait, thiserror; Feature `testing`, das `testing.rs` einbindet; die eigenen Tests aktivieren es über `[dev-dependencies] sshcraft-ssh = { path = ".", features = ["testing"] }`)

**Interfaces:**
- Consumes: `russh::keys::PrivateKey`; `sshcraft_core::policy::Disconnect`.
- Produces (`sshcraft_ssh`):
  - `struct ConnectParams { address: String, port: u16, username: String, key: russh::keys::PrivateKey }`
  - `#[async_trait] trait HostKeyVerifier: Send + Sync { async fn verify(&self, address: &str, port: u16, fingerprint: &str) -> bool; }` (Fingerprint als `SHA256:…`)
  - `async fn connect(params: ConnectParams, verifier: Arc<dyn HostKeyVerifier>) -> Result<Session, SshError>`; russh-Client-Config mit `keepalive_interval = 15 s`, `keepalive_max = 3`.
  - `Session::open_shell(&self, cols: u32, rows: u32) -> Result<Shell, SshError>` (PTY `xterm-256color`), `Session::disconnect(self)`
  - `Shell { pub events: tokio::sync::mpsc::Receiver<ShellEvent>, .. }`, `Shell::write(&self, data: &[u8]) -> Result<(), SshError>`, `Shell::resize(&self, cols: u32, rows: u32) -> Result<(), SshError>`
  - `enum ShellEvent { Data(Vec<u8>), Ended(Disconnect) }`
  - `enum SshError { HostKeyRejected, AuthFailed, Connect(String), Protocol(String) }` und `impl From<&SshError> for Disconnect`
- Test-Hilfe `sshcraft_ssh::testing` (Feature `testing`): `TestServer::start(authorized: russh::keys::PublicKey) -> TestServer { port: u16, host_fingerprint: String }`, `TestServer::stop(&mut self)`, `TestServer::restart(&mut self, new_host_key: bool)` (gleicher Port; bei `true` neuer Host-Schlüssel und neuer `host_fingerprint`). Der Server nimmt nur den einen Schlüssel an, die Shell gibt jede Eingabe zurück, auf `exit\n` sendet er Exit-Status 0 und schließt, auf `flood\n` sendet er 5 MiB festes Muster (Bytes `0..=255` wiederholt, darin die UTF-8-Folge von `"äöü🙂"`), `stop` trennt die TCP-Verbindung hart.

- [ ] **Step 1: Failing tests** in `tests/session.rs`:

```rust
#[tokio::test] async fn connects_and_echoes() { /* write b"hi\n", events yield Data containing "hi" */ }
#[tokio::test] async fn rejects_wrong_client_key() { /* other key -> Err(SshError::AuthFailed) */ }
#[tokio::test] async fn verifier_false_aborts() { /* verifier returns false -> Err(SshError::HostKeyRejected) */ }
#[tokio::test] async fn verifier_sees_server_fingerprint() { /* fingerprint passed == server.host_fingerprint */ }
#[tokio::test] async fn exit_yields_exited() { /* write b"exit\n" -> Ended(Disconnect::Exited(Some(0))) */ }
#[tokio::test] async fn server_drop_yields_lost() { /* server.stop() -> Ended(Disconnect::Lost(_)) within 5 s */ }
#[tokio::test] async fn large_output_arrives_complete_and_in_order() { /* b"flood\n": concatenated Data == expected 5 MiB pattern */ }
```

- [ ] **Step 2:** `cargo test -p sshcraft-ssh` schlägt fehl.
- [ ] **Step 3:** `connect`, `Session`, `Shell` umsetzen; Ausgabe des Kanals als rohe Bytes weiterreichen, nicht in Text umwandeln (UTF-8 über Paketgrenzen löst xterm.js auf). Kanal-Puffer 1024 Nachrichten.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenprobe:** in `check_server_key` immer `true` zurückgeben statt den Verifier zu fragen: `verifier_false_aborts` muss rot werden. Zurücknehmen.
- [ ] **Step 6:** Commit `SSH-Sitzungen über russh mit Keep-Alive`.

---

### Task 6: Tauri-Befehle und Sitzungsverwaltung

**Files:**
- Create: `app/src-tauri/src/state.rs`, `app/src-tauri/src/sessions.rs`, `app/src-tauri/src/commands/{mod.rs,data.rs,keys.rs,sessions.rs}`
- Modify: `app/src-tauri/src/lib.rs`, `app/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: alles aus Task 2 bis 5.
- Produces (Tauri-Befehle, Namen in snake_case, Fehler als `{ code: string, message: string }`):
  - Daten: `list_groups() -> Group[]`, `save_group(group)`, `delete_group(id)`, `list_hosts() -> Host[]`, `save_host(host)`, `delete_host(id)`, `forget_host_key(host_id)`
  - Schlüssel: `list_keys() -> KeyInfo[]`, `import_key(name, text, passphrase?) -> KeyInfo`, `import_key_file(name, path, passphrase?) -> KeyInfo`, `generate_key(name) -> KeyInfo`, `delete_key(id)`
  - Sitzungen: `session_open(host_id, cols, rows, on_data: Channel<InvokeResponseBody>, on_status: Channel<SessionStatus>) -> SessionId`, `session_write(id, data: Vec<u8>)`, `session_resize(id, cols, rows)`, `session_close(id)`
  - `SessionStatus` (serde-Tag `"state"`): `Connecting`, `Connected`, `Reconnecting { attempt, delay_secs }`, `Ended { reason: "exited" | "lost" | "host_key_rejected" | "host_key_mismatch" | "auth_failed", detail: Option<String> }`
  - Host-Key-Rückfrage: Ereignis `host-key-prompt` mit `{ prompt_id, host_name, address, port, fingerprint }` nur bei `Unknown`; Antwort über Befehl `answer_host_key(prompt_id, accept: bool)`. Bei `Mismatch` keine Rückfrage, sondern `Ended { reason: "host_key_mismatch", detail: "expected …, got …" }`.
- `on_data` schickt rohe Bytes (`InvokeResponseBody::Raw`), kein JSON-Array.
- `SessionManager::open` löst den Host auf, lädt den Schlüssel aus dem Tresor, verbindet, öffnet die Shell; bei `Ended(Lost)` fragt er `next_reconnect_delay`, meldet `Reconnecting`, wartet, verbindet neu (mit derselben Fingerprint-Prüfung gegen den gespeicherten Wert) und öffnet die Shell in derselben Größe.
- Beim Start: Datenbank unter `directories::ProjectDirs::from("de", "musiker15", "SSHCraft").data_dir()/sshcraft.db`, `Vault::unlock` mit `KeyringMasterKey`. Schlägt `unlock` mit `MasterKeyMissing` fehl, startet die App trotzdem und die Oberfläche zeigt einen Fehlerzustand mit Erklärung (Befehl `vault_status() -> { ok: bool, error?: string }`).

- [ ] **Step 1: Failing tests** in `sessions.rs` (Rust, gegen den `TestServer` aus Task 5; `app/src-tauri/Cargo.toml` bekommt `[dev-dependencies] sshcraft-ssh = { path = "../../crates/sshcraft-ssh", features = ["testing"] }`):

```rust
#[tokio::test] async fn unknown_host_key_asks_and_stores_on_accept() { /* prompt answered true -> known_host stored, Connected */ }
#[tokio::test] async fn unknown_host_key_declined_ends_rejected() { /* answer false -> Ended host_key_rejected, nothing stored */ }
#[tokio::test] async fn mismatch_ends_without_prompt() { /* stored "SHA256:other" -> no prompt, Ended host_key_mismatch */ }
#[tokio::test] async fn lost_connection_reconnects() { /* server restarted on same port with same host key -> Reconnecting then Connected */ }
#[tokio::test] async fn reconnect_into_changed_host_key_stops() { /* server restarted with new host key -> Ended host_key_mismatch, no further attempts */ }
#[tokio::test] async fn exit_does_not_reconnect() { /* "exit" -> Ended exited, no Reconnecting */ }
```

Der `SessionManager` bekommt Ausgabe und Rückfragen über ein Trait `SessionSink` (Implementierung mit Tauri-Channels in der App, mit Vektoren im Test), damit die Tests ohne Tauri-Laufzeit auskommen. Die Wartezeiten kommen aus `next_reconnect_delay`; im Test über einen Faktor `delay_scale: f32` auf 0,01 verkürzt.

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** `state.rs`, `sessions.rs` und die Befehle umsetzen; `lib.rs` registriert alles.
- [ ] **Step 4:** `cargo test -p sshcraft-app` grün.
- [ ] **Step 5: Gegenprobe:** beim Wiederverbinden die Fingerprint-Prüfung auslassen: `reconnect_into_changed_host_key_stops` muss rot werden. Zurücknehmen.
- [ ] **Step 6:** Commit `Tauri-Befehle und Sitzungsverwaltung mit Wiederverbinden`.

---

### Task 7: Oberfläche

**Files:**
- Create: `app/src/api.ts`, `app/src/state/tabs.ts`, `app/src/state/tabs.test.ts`, `app/src/forms/hostForm.ts`, `app/src/forms/hostForm.test.ts`, `app/src/components/{Sidebar,GroupForm,HostForm,KeyManager,TerminalTabs,TerminalView,HostKeyDialog}.tsx`
- Modify: `app/src/App.tsx`, `app/src/main.tsx`, `app/vite.config.ts` (Vitest mit `environment: "jsdom"`)

**Interfaces:**
- Consumes: Befehle und Ereignisse aus Task 6. `api.ts` spiegelt die Rust-Typen als TypeScript-Typen (`Group`, `Host`, `ConnectionSettings`, `KeyRef`, `KeyInfo`, `SessionStatus`) und kapselt jeden Befehl als Funktion gleichen Namens in camelCase (`listGroups`, `saveHost`, `sessionOpen`, …).
- Produces:
  - `state/tabs.ts`: `type Tab = { id: string; hostId: string; title: string; sessionId?: string; status: SessionStatus }`, `tabsReducer(state: TabsState, action: TabsAction): TabsState` mit Aktionen `open`, `attach` (sessionId setzen), `status`, `close`, `activate`.
  - `forms/hostForm.ts`: `toConnectionSettings(form: HostFormValues): ConnectionSettings` (leere Felder werden `null`, damit geerbt wird; Port als Zahl 1 bis 65535, sonst Fehler), `fromConnectionSettings(settings): HostFormValues`.

- [ ] **Step 1: Failing tests:**

```ts
test("empty fields become null so the group value is inherited", () => { /* address "" -> null */ });
test("port outside 1..65535 is rejected", () => { /* "0" and "70000" -> error */ });
test("opening a tab activates it", () => { /* open -> activeId === new tab id */ });
test("closing the active tab activates its neighbour", () => {});
test("status updates only touch the matching tab", () => {});
```

- [ ] **Step 2:** `npm test` schlägt fehl.
- [ ] **Step 3:** `hostForm.ts` und `tabs.ts` umsetzen, Tests grün.
- [ ] **Step 4:** Komponenten bauen:
  - Seitenleiste mit Gruppen (aufklappbar) und Hosts ohne Gruppe; Doppelklick verbindet; Kontextmenü Bearbeiten, Löschen (mit Bestätigung).
  - Host- und Gruppenformular; im Host zeigen leere Felder den geerbten Wert als Platzhalter; Knopf "Fingerprint vergessen".
  - Schlüsselverwaltung: Liste (Name, Typ, Fingerprint, öffentlicher Schlüssel zum Kopieren), Import per Datei oder Einfügen, Passphrase-Feld, Erzeugen.
  - Terminal-Tabs mit xterm.js und `@xterm/addon-fit`; Bytes aus `on_data` gehen als `Uint8Array` direkt an `terminal.write`; Eingaben über `session_write`; Größenänderung über `session_resize`.
  - Statuszeile im Tab: Verbinden, Verbindung verloren mit Countdown, beendet mit Grund; bei `host_key_mismatch` ein deutlicher Warnhinweis mit beiden Fingerprints und dem Hinweis auf "Fingerprint vergessen".
  - Dialog für unbekannte Host-Fingerprints mit Annehmen und Ablehnen.
  - Fehlerzustand bei `vault_status().ok === false` mit Erklärung, dass der Hauptschlüssel in der Windows-Anmeldeinformationsverwaltung fehlt.
  - Dunkles Erscheinungsbild als Vorgabe; Gestaltung im Einzelnen frei.
- [ ] **Step 5:** `npm run typecheck && npm test` grün.
- [ ] **Step 6: Prüfung von Hand** mit `npm run tauri dev` gegen einen echten Server:
  - Gruppe mit Adresse und Benutzer, darin zwei Hosts mit unterschiedlichen Schlüsseln; beide verbinden.
  - Erstverbindung zeigt Fingerprint-Dialog; zweite Verbindung fragt nicht mehr.
  - `cat` einer Datei mit Umlauten und Emojis wird richtig angezeigt; `seq 1 2000000` hält die Oberfläche bedienbar.
  - WLAN kurz trennen: Tab zeigt "Verbindung verloren", verbindet neu.
  - `exit`: Tab zeigt "beendet", kein neuer Versuch.
  - In der Windows-Anmeldeinformationsverwaltung den Eintrag `de.musiker15.sshcraft` umbenennen, App neu starten: Fehlerzustand statt leerer Schlüsselliste; Eintrag zurückbenennen, alles wieder da.
- [ ] **Step 7:** `npm run tauri build` erzeugt einen Windows-Installer.
- [ ] **Step 8:** Commit `Oberfläche mit Seitenleiste, Schlüsselverwaltung und Terminal-Tabs`.
