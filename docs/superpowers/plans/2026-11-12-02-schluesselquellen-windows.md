# SSHCraft Teilplan 2: Schlüsselquellen und Windows-Anbindung Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Die Windows-App verbindet sich mit Schlüsseln aus dem Proton-Pass-Agenten, importiert Schlüssel über `pass-cli` und Hosts aus `~/.ssh/config`, und gespeicherte Schlüssel werden nach dem Start erst nach Windows Hello benutzt.

**Architecture:** `KeyRef` bekommt die Variante `Agent { fingerprint }`. `sshcraft-ssh` meldet sich wahlweise mit einem privaten Schlüssel oder über einen russh-`AgentClient` an; der Agent kommt über ein Trait, damit Tests einen russh-Agent-Server im Speicher nutzen. `pass-cli` und Windows Hello stecken hinter Traits in der App, `~/.ssh/config` wird in `sshcraft-core` als reine Funktion geparst. Die Sitzungsverwaltung fragt Windows Hello einmal pro App-Lauf, bevor sie einen gespeicherten Schlüssel aus dem Tresor holt.

**Tech Stack:** wie Teilplan 1, dazu `windows` 0.62.2 (Features `Security_Credentials_UI`, `Foundation`, `Win32_Foundation`, `Win32_System_WinRT`), `pass-cli` 2.7.0 (Proton Pass, nicht mitgeliefert).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md` (Abschnitte 5, 9, 10, 11), Fahrplan `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md`, Vorgänger `docs/superpowers/plans/2026-11-11-01-desktop-grundlage.md`.

## Global Constraints

- Alle Regeln aus Teilplan 1 gelten weiter (Arbeitsbäume unter `G:\Claude\worktrees`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8, neueste stabile Versionen, Gegenproben für Regeln, Rechte und Datenwege).
- Agent-Pipe: `\\.\pipe\openssh-ssh-agent`. Ein privater Schlüssel aus dem Agenten verlässt Proton Pass nie; SSHCraft speichert nur den Fingerprint (`SHA256:…`).
- `pass-cli`: zuerst `%LOCALAPPDATA%\Programs\ProtonPass\pass-cli.exe`, sonst `pass-cli` aus dem `PATH`. `item view` immer mit `PROTON_PASS_AGENT_REASON="SSHCraft: import an SSH key into the encrypted local key vault"`. Die Ausgabe von `item view` wird nie geloggt, nie in eine Datei geschrieben, nie in eine Fehlermeldung übernommen und liegt nur in `Zeroizing`-Puffern.
- Feldpfade (geprüft am 12.11.2026 mit pass-cli 2.7.0): Liste `items[].{id, share_id, title, item_type}`, SSH-Schlüssel haben `item_type == "ssh_key"`; Einzelansicht `item.content.content.SshKey.private_key`.
- Windows Hello schützt nur Schlüssel aus dem Tresor (`KeyRef::Stored`). Agent-Schlüssel brauchen es nicht, Proton Pass schützt sie selbst.
- Keine Gruppe und kein Host wird beim ssh-config-Import ohne Bestätigung in der Vorschau angelegt.

## Review Focus

1. **Proton Pass läuft nicht oder die Pipe fehlt:** Verbinden endet nach höchstens 5 s mit "Proton Pass ist nicht erreichbar", kein Hängen, kein Wiederverbinden-Kreislauf. Test in Task 2.
2. **Der Agent verweigert die Signatur** (gesperrter Tresor, abgelehnte Bestätigung) **oder kennt den Schlüssel nicht mehr:** verständliche Meldung, kein Absturz, kein endloses Warten. Tests in Task 1.
3. **`pass-cli` fehlt, ist abgemeldet, ein Eintrag hat keinen privaten Schlüssel oder ist mit Passphrase geschützt:** verständliche Meldung; der private Schlüssel steht in keiner Meldung. Tests in Task 3.
4. **Unordentliche `~/.ssh/config`:** BOM, CRLF, `Key=Value`, Anführungszeichen, Pfade mit Leerzeichen und `~`, `Include`, `Match`, Muster mit `*`, `?`, `!`: kein Absturz, `Match` und `Include` werden übersprungen und in der Vorschau genannt. Tests in Task 4.
5. **Windows Hello abgebrochen oder nicht eingerichtet** (auch per Remotedesktop): kein Aussperren, klare Meldung; Agent-Schlüssel funktionieren trotzdem. Tests in Task 5.

---

## Dateistruktur

```
crates/sshcraft-core/src/
  model.rs            + KeyRef::Agent { fingerprint }
  sshconfig.rs (neu)  parse(), ConfigHost, plan_import(), ImportCandidate
  sshconfig_tests.rs (neu)
crates/sshcraft-ssh/src/
  lib.rs              Credential, DynAgent, list_agent_keys(), AgentKey, neue SshError-Varianten
  agent.rs (neu)      open_windows_agent()
  testing.rs          + spawn_agent()
app/src-tauri/src/
  sessions.rs         Credential-Auflösung, AgentConnector, KeyGate (Windows Hello)
  passcli.rs (neu)    PassCli-Trait, PassCliProcess, parse_key_list(), parse_private_key(), import_from_pass_cli()
  hello.rs (neu)      UserPresence-Trait, WindowsHello, Presence
  commands/keys.rs    list_agent_keys, pass_cli_status, pass_cli_list_keys, import_from_pass_cli; Import async
  commands/data.rs    ssh_config_preview, ssh_config_import
app/src/
  api.ts, forms/hostForm.ts (+ test), components/SettingsFields.tsx, KeyManager.tsx,
  SshConfigImport.tsx (neu), App.tsx
```

---

### Task 1: Anmeldung über einen SSH-Agenten in `sshcraft-ssh`

**Files:**
- Modify: `crates/sshcraft-core/src/model.rs`, `crates/sshcraft-ssh/src/lib.rs`, `crates/sshcraft-ssh/src/testing.rs`, `crates/sshcraft-ssh/tests/session.rs`
- Create: `crates/sshcraft-ssh/src/agent.rs`

**Interfaces:**
- Consumes: `connect`, `ConnectParams`, `HostKeyVerifier`, `SshError` aus Teilplan 1.
- Produces:
  - `sshcraft_core::model::KeyRef::Agent { fingerprint: String }` (serde `{ "kind": "agent", "fingerprint": "SHA256:…" }`); `resolve` behandelt sie wie `Stored`.
  - `pub type DynAgent = russh::keys::agent::client::AgentClient<Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>>;`
  - `pub enum Credential { Key(PrivateKey), Agent { fingerprint: String, agent: DynAgent } }`; `ConnectParams.key` wird zu `ConnectParams.credential: Credential`.
  - `pub struct AgentKey { pub fingerprint: String, pub algorithm: String, pub comment: String, pub public_key: String }` (`Serialize`, `Clone`, `PartialEq`).
  - `pub async fn list_agent_keys(agent: &mut DynAgent) -> Result<Vec<AgentKey>, SshError>`
  - `agent::open_windows_agent() -> Result<DynAgent, SshError>` (nur `cfg(windows)`), Zeitlimit 5 s, Pipe aus Global Constraints.
  - Neue Varianten `SshError::AgentUnavailable(String)`, `SshError::AgentKeyMissing`, `SshError::AgentRefused`; `From<&SshError> for Disconnect` bildet alle drei auf `Disconnect::AuthFailed` ab (kein Wiederverbinden).
  - `testing::spawn_agent(keys: &[PrivateKey], confirm: bool) -> DynAgent`: russh-Agent-Server über `tokio::io::duplex`, `confirm == false` lässt jede Signatur ablehnen.

- [ ] **Step 1: Failing tests** in `tests/session.rs`:

```rust
#[tokio::test] async fn connects_with_agent_key() { /* agent holds k, credential Agent{fingerprint of k} -> Ok */ }
#[tokio::test] async fn agent_without_that_key_fails() { /* agent holds other key -> Err(SshError::AgentKeyMissing) */ }
#[tokio::test] async fn agent_refusing_to_sign_fails() { /* spawn_agent(&[k], false) -> Err(SshError::AgentRefused) within 5 s */ }
#[tokio::test] async fn list_agent_keys_returns_fingerprints() { /* two keys -> both SHA256 fingerprints, algorithm "ssh-ed25519" */ }
```

und in `model.rs`: `agent_key_ref_roundtrips_json` (`{"kind":"agent","fingerprint":"SHA256:x"}`), `host_inherits_agent_key_from_group`.

- [ ] **Step 2:** `cargo test -p sshcraft-ssh -p sshcraft-core` schlägt fehl.
- [ ] **Step 3:** Umsetzen. Anmeldung mit Agent: Identität per Fingerprint aus `request_identities()` suchen, dann `authenticate_publickey_with(user, identity.public_key().into_owned(), best_supported_rsa_hash, &mut agent)`; ein Signaturfehler des Agenten wird `AgentRefused`, eine fehlende Identität `AgentKeyMissing`.
- [ ] **Step 4:** Tests grün, die bestehenden Tests aus Teilplan 1 bleiben grün (`ConnectParams` in den Tests auf `Credential::Key` umstellen).
- [ ] **Step 5: Gegenprobe:** in der Identitätssuche den ersten Schlüssel nehmen statt per Fingerprint zu suchen: `agent_without_that_key_fails` muss rot werden.
- [ ] **Step 6:** Commit `Anmeldung über einen SSH-Agenten`.

---

### Task 2: Agent-Schlüssel in der App

**Files:**
- Modify: `app/src-tauri/src/sessions.rs`, `app/src-tauri/src/sessions_tests.rs`, `app/src-tauri/src/state.rs`, `app/src-tauri/src/commands/keys.rs`, `app/src-tauri/src/lib.rs`, `app/src/api.ts`, `app/src/forms/hostForm.ts`, `app/src/forms/hostForm.test.ts`, `app/src/components/SettingsFields.tsx`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `#[async_trait] pub trait AgentConnector: Send + Sync + 'static { async fn connect(&self) -> Result<DynAgent, SshError>; }`, `WindowsAgent` (ruft `open_windows_agent`), im Test ein Connector auf `spawn_agent`.
  - `SessionManager::with_delay_scale(store, vault, prompter, agent: Arc<dyn AgentConnector>, delay_scale)` und `new(…, agent)`; `connect_once` baut aus `KeyRef` das `Credential`.
  - Abbildung: `AgentUnavailable` → `Ended { reason: Error, detail: "Proton Pass ist nicht erreichbar. Läuft die Proton-Pass-App?" }`, `AgentKeyMissing` → `Error` mit "Der Schlüssel ist nicht mehr in Proton Pass.", `AgentRefused` → `AuthFailed` mit "Proton Pass hat die Signatur abgelehnt."
  - Tauri-Befehl `list_agent_keys() -> AgentKey[]` (Fehler `agent_unavailable`).
  - `api.ts`: `KeyRef = { kind: "stored"; id: string } | { kind: "agent"; fingerprint: string }`, `AgentKey`, `listAgentKeys()`.
  - `hostForm.ts`: `SettingsFormValues.keyId` wird `keyChoice: string` mit den Formen `""`, `"stored:<uuid>"`, `"agent:<fingerprint>"`; `toConnectionSettings` und `fromConnectionSettings` bilden das ab.

- [ ] **Step 1: Failing tests:**

```rust
#[tokio::test] async fn agent_key_connects() { /* host with KeyRef::Agent, connector over spawn_agent -> Connected */ }
#[tokio::test] async fn agent_unavailable_ends_with_error_and_no_retry() { /* connector returns AgentUnavailable -> Ended Error, detail contains "Proton Pass", no Reconnecting */ }
#[tokio::test] async fn agent_key_needs_no_vault() { /* SessionManager with vault None + agent host -> Connected */ }
```

```ts
test("agent choice maps to an agent key ref", () => { /* "agent:SHA256:x" -> { kind: "agent", fingerprint: "SHA256:x" } */ });
test("stored choice maps to a stored key ref", () => { /* "stored:k1" -> { kind: "stored", id: "k1" } */ });
test("fingerprints containing colons survive the round trip", () => {});
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen. `SettingsFields` zeigt zwei `<optgroup>`: "Proton Pass" (aus `listAgentKeys`, bei Fehler ein deaktivierter Eintrag "Proton Pass nicht erreichbar") und "Gespeichert". Ein Host, dessen Agent-Fingerprint nicht in der Liste ist, behält seinen Wert und zeigt "nicht gefunden".
- [ ] **Step 4:** `cargo test -p sshcraft-app --lib`, `npm run typecheck && npm test` grün.
- [ ] **Step 5: Gegenprobe:** Agent-Fehler als `Lost` statt `Error` abbilden: `agent_unavailable_ends_with_error_and_no_retry` muss rot werden.
- [ ] **Step 6:** Commit `Schlüssel aus dem Proton-Pass-Agenten in Hosts und Gruppen`.

---

### Task 3: Import über `pass-cli`

**Files:**
- Create: `app/src-tauri/src/passcli.rs`, `app/src-tauri/src/passcli_tests.rs`
- Modify: `app/src-tauri/src/commands/keys.rs`, `app/src-tauri/src/lib.rs`, `app/src/api.ts`, `app/src/components/KeyManager.tsx`

**Interfaces:**
- Consumes: `Vault::import_key` (Teilplan 1), `KeyInfo`.
- Produces:
  - `pub struct PassKey { pub share_id: String, pub item_id: String, pub title: String }` (`Serialize`)
  - `pub enum PassCliStatus { NotInstalled, LoggedOut, Ready }` (serde snake_case)
  - `pub trait PassCli: Send + Sync { fn status(&self) -> PassCliStatus; fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError>; fn private_key(&self, share_id: &str, item_id: &str) -> Result<Zeroizing<String>, PassCliError>; }`
  - `PassCliProcess` (echte Umsetzung über `std::process::Command`, `CREATE_NO_WINDOW`), `status` über `pass-cli info` (Exit-Code).
  - `pub fn parse_key_list(json: &str) -> Result<Vec<PassKey>, PassCliError>` (nur `item_type == "ssh_key"`), `pub fn parse_private_key(json: &str) -> Result<Zeroizing<String>, PassCliError>`
  - `pub fn import_from_pass_cli(cli: &dyn PassCli, vault: &Vault, store: &Store, share_id: &str, item_id: &str, name: &str) -> Result<KeyInfo, PassCliError>`
  - `enum PassCliError { NotInstalled, LoggedOut, NoPrivateKey, Encrypted, Failed(String), Vault(VaultError) }`, deutsche `Display`-Texte; `Failed` trägt nur den Exit-Code und die erste Zeile von stderr, nie stdout.
  - Befehle `pass_cli_status`, `pass_cli_list_keys`, `import_from_pass_cli(shareId, itemId, name)`; alle Importbefehle (`import_key`, `generate_key`, `import_from_pass_cli`) werden `async` und laufen in `tokio::task::spawn_blocking`, der Store wird erst nach dem Entschlüsseln gesperrt (behebt den zurückgestellten Review-Befund "Import blockiert das Fenster"). `import_key_file` wird entfernt (zurückgestellter Review-Befund).

- [ ] **Step 1: Failing tests** in `passcli_tests.rs`, mit Fixture-JSON in den Tests (keine echte pass-cli):

```rust
#[test] fn key_list_keeps_only_ssh_keys() { /* items with item_type ssh_key and custom -> only ssh_key */ }
#[test] fn private_key_is_read_from_ssh_key_content() { /* item.content.content.SshKey.private_key */ }
#[test] fn item_without_private_key_is_an_error() { /* -> Err(NoPrivateKey) */ }
#[test] fn import_stores_the_key_encrypted() { /* FakePassCli returns an OpenSSH key -> KeyInfo fingerprint matches, vault can load it */ }
#[test] fn passphrase_protected_key_reports_encrypted() { /* -> Err(Encrypted) */ }
#[test] fn errors_never_contain_the_private_key() {
    // FakePassCli returns a broken key that still contains a known marker line;
    // the Display text of the error must not contain the marker.
}
#[test] fn logged_out_cli_reports_logged_out() { /* FakePassCli status LoggedOut -> list returns Err(LoggedOut) */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen. `KeyManager` bekommt einen Bereich "Aus Proton Pass": Status (nicht installiert mit Link-Text auf die Installationsanleitung, abgemeldet mit Hinweis `pass-cli login`), Liste der Titel, Knopf "Importieren" je Eintrag (Name vorbelegt mit dem Titel). Neben dem Bereich der Hinweis, dass Schlüssel, die nur zum Verbinden gebraucht werden, nicht importiert werden müssen, weil SSHCraft den Proton-Pass-Agenten direkt nutzt.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenprobe:** in `PassCliError::Failed` stdout statt stderr übernehmen: `errors_never_contain_the_private_key` muss rot werden.
- [ ] **Step 6:** Commit `Schlüssel aus Proton Pass über pass-cli importieren`.

---

### Task 4: Import aus `~/.ssh/config`

**Files:**
- Create: `crates/sshcraft-core/src/sshconfig.rs`, `crates/sshcraft-core/src/sshconfig_tests.rs`, `app/src/components/SshConfigImport.tsx`
- Modify: `crates/sshcraft-core/src/lib.rs`, `app/src-tauri/src/commands/data.rs`, `app/src-tauri/src/lib.rs`, `app/src/api.ts`, `app/src/App.tsx`, `app/src/components/Sidebar.tsx`

**Interfaces:**
- Produces (`sshcraft_core::sshconfig`):
  - `pub struct ConfigHost { pub alias: String, pub hostname: Option<String>, pub port: Option<u16>, pub user: Option<String>, pub identity_files: Vec<String> }`
  - `pub struct ParsedConfig { pub hosts: Vec<ConfigHost>, pub skipped: Vec<String> }` (`skipped`: Zeilen mit `Include`/`Match`, je als kurzer Text)
  - `pub fn parse(text: &str) -> ParsedConfig`: OpenSSH-Regel "der erste gefundene Wert gilt", Werte aus passenden Muster-Blöcken (`Host *`, `Host *.example.org`) füllen nur, was der Alias selbst nicht setzt; Aliase mit `*`, `?` oder `!` werden selbst nicht importiert; Schlüsselwörter ohne Rücksicht auf Groß- und Kleinschreibung, `Key Value` und `Key=Value`, Anführungszeichen, BOM und CRLF.
  - `pub struct ImportCandidate { pub alias: String, pub group: Option<String>, pub settings: ConnectionSettings, pub key_note: Option<String> }` (`Serialize`)
  - `pub fn plan_import(hosts: &[ConfigHost], fingerprint_of: &dyn Fn(&str) -> Option<String>, agent_fingerprints: &[String]) -> Vec<ImportCandidate>`: `IdentityFile` → `fingerprint_of` (liest `<pfad>` wenn er auf `.pub` endet, sonst `<pfad>.pub`, `~` = Benutzerordner) → liegt der Fingerprint im Agenten, `KeyRef::Agent`, sonst kein Schlüssel und `key_note = "Schlüssel nicht in Proton Pass, bitte zuordnen"`. Mehrere Aliase mit demselben `HostName` landen in einer Gruppe, die nach dem `HostName` heißt, und die Adresse steht an der Gruppe; ein einzelner Alias bleibt ohne Gruppe.
- Befehle `ssh_config_preview() -> { candidates: ImportCandidate[], skipped: string[] }` und `ssh_config_import(aliases: string[]) -> { created: number }`; ein Alias, zu dem es schon einen Host mit gleichem Namen und gleicher Adresse gibt, wird übersprungen.

- [ ] **Step 1: Failing tests** in `sshconfig_tests.rs`:

```rust
#[test] fn parses_basic_host_block() {}
#[test] fn first_value_wins_and_star_block_fills_gaps() { /* Host a sets User root; Host * sets User x, Port 2222 -> user root, port 2222 */ }
#[test] fn equals_sign_quotes_bom_and_crlf() {}
#[test] fn wildcard_and_negated_aliases_are_not_imported() {}
#[test] fn include_and_match_are_skipped_and_reported() {}
#[test] fn multiple_aliases_on_one_host_line() { /* "Host a b" -> two hosts */ }
#[test] fn identity_file_with_pub_maps_to_agent_key() { /* Moritz' style: IdentityFile ~/.ssh/x.pub, IdentitiesOnly yes */ }
#[test] fn unknown_key_gets_a_note() {}
#[test] fn same_hostname_becomes_one_group() {}
#[test] fn garbage_does_not_panic() { /* random bytes, lone keywords, port "abc" -> no panic, port None */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen, dazu `SshConfigImport.tsx`: Vorschau als Liste mit Haken (Vorgabe: alle an), Gruppe, Adresse, Benutzer, Schlüssel oder Hinweis; darunter die übersprungenen Zeilen; Knopf "Importieren". Einstieg über einen Knopf "Aus ssh-config" in der Seitenleiste.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5:** Commit `Hosts aus ~/.ssh/config importieren`.

---

### Task 5: Windows Hello vor gespeicherten Schlüsseln

**Files:**
- Create: `app/src-tauri/src/hello.rs`
- Modify: `app/src-tauri/src/sessions.rs`, `app/src-tauri/src/sessions_tests.rs`, `app/src-tauri/src/state.rs`, `app/src-tauri/Cargo.toml`, `app/src/App.tsx`

**Interfaces:**
- Produces:
  - `pub enum Presence { Verified, Declined, Unavailable }`
  - `#[async_trait] pub trait UserPresence: Send + Sync + 'static { async fn verify(&self, reason: &str) -> Presence; }`
  - `WindowsHello { hwnd }`: `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` mit dem Fenster-Handle des Hauptfensters (sonst erscheint der Dialog hinter dem Fenster). `UserConsentVerifierAvailability` ungleich `Available` → `Unavailable`.
  - `KeyGate` in `sessions.rs`: hält den Zustand pro App-Lauf (`tokio::sync::Mutex<bool>`); `async fn ensure(&self) -> Result<(), String>`: schon verifiziert → Ok; sonst `verify("SSHCraft möchte deine gespeicherten SSH-Schlüssel verwenden.")`; `Verified` merkt sich das; `Unavailable` lässt durch und setzt `hello_missing = true`; `Declined` → `Err("Windows Hello wurde abgebrochen.")`, beim nächsten Versuch wird wieder gefragt. Gleichzeitige Aufrufe teilen sich eine Abfrage.
  - `SessionManager::new` und `with_delay_scale` bekommen nach `agent` den Parameter `presence: Arc<dyn UserPresence>`; die Tests aus Teilplan 1 und Task 2 nutzen eine `FakePresence` mit `Verified`.
  - `connect_once` ruft `ensure` nur für `KeyRef::Stored`, vor `vault.private_key`. Abbruch → `Ended { reason: Error, … }`, kein Wiederverbinden.
  - Befehl `hello_status() -> { missing: bool }`; `App.tsx` zeigt bei `missing` einmal einen Hinweis: "Windows Hello ist auf diesem PC nicht eingerichtet. Gespeicherte Schlüssel sind dann nur durch dein Windows-Konto geschützt."
  - **[Claudes Entscheidung]** Ohne eingerichtetes Windows Hello wird nicht gesperrt, sondern gewarnt; sonst wäre SSHCraft auf solchen PCs und in Remotedesktop-Sitzungen unbenutzbar.

- [ ] **Step 1: Failing tests** in `sessions_tests.rs` mit einer `FakePresence` (Antwort und Zähler):

```rust
#[tokio::test] async fn stored_key_asks_hello_once_per_run() { /* two sessions, Verified -> asked == 1 */ }
#[tokio::test] async fn declined_hello_ends_without_connecting() { /* Declined -> Ended Error, server saw no connection, no Reconnecting */ }
#[tokio::test] async fn declined_hello_asks_again_next_time() { /* Declined then Verified -> second session connects, asked == 2 */ }
#[tokio::test] async fn unavailable_hello_connects_and_flags() { /* Unavailable -> Connected, gate.hello_missing() == true */ }
#[tokio::test] async fn agent_key_does_not_ask_hello() { /* KeyRef::Agent -> asked == 0 */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen; `windows` mit den Features aus dem Kopf als `cfg(windows)`-Abhängigkeit.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenprobe:** `ensure` vor `vault.private_key` auskommentieren: `declined_hello_ends_without_connecting` muss rot werden.
- [ ] **Step 6:** Commit `Windows Hello vor gespeicherten Schlüsseln`.

---

### Task 6: Prüfung von Hand und Installer

**Files:** keine neuen; `README.md` bekommt in der Tabelle "Documentation" keinen neuen Eintrag, aber unter "Quick start" einen Satz zu Proton Pass.

- [ ] **Step 1:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cd app && npm run typecheck && npm test` grün.
- [ ] **Step 2: Von Hand** mit `npm run tauri dev` (Moritz):
  - Proton Pass läuft: Host mit Agent-Schlüssel verbinden; Proton Pass beenden, erneut verbinden: Meldung "nicht erreichbar" nach höchstens 5 s.
  - `pass-cli`: Schlüssel importieren, Host damit verbinden; beim ersten Mal erscheint Windows Hello, beim zweiten Host nicht mehr; Hello abbrechen: Meldung, kein Wiederverbinden.
  - `~/.ssh/config` von Moritz: Vorschau zeigt die Hosts mit Proton-Pass-Schlüsseln, übersprungene Zeilen sind genannt; Import legt Gruppen nach `HostName` an.
- [ ] **Step 3:** `npm run tauri build`, Installer unter `target\release\bundle\nsis\`.
- [ ] **Step 4:** Commit `README: Proton Pass im Schnellstart`.
