# SSHCraft Teilplan 9: Verbindungen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hosts verbinden sich außer mit Schlüssel auch mit Passwort und keyboard-interactive, über Identitäten, Sprung-Hosts sowie HTTP- und SOCKS5-Proxys, mit Agent-Weiterleitung und Umgebungsvariablen. Dazu kommen Tabs für die serielle Verbindung und das lokale Terminal.

**Architecture:** `ConnectionSettings` bekommt Passwort, Identität, Sprung-Hosts, Proxy, Agent-Weiterleitung und Umgebungsvariablen, alle über die Gruppenkette vererbt. Eine Identität füllt auf ihrer Ebene (Host oder Gruppe) die Felder, die diese Ebene nicht selbst setzt. Passwörter liegen versiegelt im Tresor, als eigene Einträge wie Schlüssel. Sie werden wie Schlüssel synchronisiert und geteilt, Identitäten laufen denselben Weg. Der SSH-Kern baut eine Route: Proxy, dann die Sprung-Hosts über `direct-tcpip`, dann das Ziel. Jeder Hop meldet sich mit seinen eigenen Zugangsdaten an und prüft seinen eigenen Host-Schlüssel. Ein neues Crate `sshcraft-local` (nur Desktop) liefert PTY und serielle Schnittstelle. Der `SessionManager` öffnet Tabs für ein Ziel `Host | Local | Serial`.

**Tech Stack:** wie Teilplan 8; neu: `tokio-socks` 0.5.3 (SOCKS5-Client), `portable-pty` 0.9.0 (ConPTY unter Windows, PTY sonst), `serialport` 4.10.1. HTTP CONNECT schreiben wir selbst, Basic-Auth über `data-encoding` (schon im Workspace). Vor dem Eintragen jeweils neu prüfen (`cargo info`). `russh` 0.65.1 bringt `mlkem768x25519-sha256` bereits als ersten Schlüsseltausch mit (geprüft am 12.11.2026 in `negotiation.rs`, `SAFE_KEX_ORDER`); ML-DSA für Host-Schlüssel kann `russh` noch nicht.

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`. Maßgeblich sind:
- Abschnitt 2 "Verbindungen": Passwort und keyboard-interactive, Passwörter verschlüsselt im Tresor und synchronisiert wie Schlüssel, Identitäten für Hosts und Gruppen, Sprung-Hosts je Host oder Gruppe vererbt, HTTP- und SOCKS5-Proxy je Host mit optionalen Zugangsdaten, Agent-Weiterleitung, Umgebungsvariablen je Host, seriell und lokales Terminal nur auf dem Desktop, Post-Quanten-Schlüsseltausch.
- Abschnitt 3: lokales Terminal unter Windows PowerShell, cmd oder WSL, sonst die Login-Shell.
- Abschnitt 8: Teilen mit oder ohne Schlüssel.
- Abschnitt 9: Host-Schlüssel, Wiederverbinden.
- Abschnitt 11: nichts Geheimes im Klartext auf der Platte oder in Logs.

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 8 gelten weiter:
  - Arbeitsbaum unter `G:\Claude\worktrees`.
  - Code auf Englisch, Commits auf Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8.
  - Neueste Paketversionen, `cargo update` ohne `--workspace`, `npm outdated`.
  - Gegenproben für Regeln, Rechte und Datenwege.
  - Ohne Remote nach grünen Tests lokal in `main` mergen.
  - Oberflächentexte nur über die Übersetzung (en, de), Fehler aus Rust nur als `ErrorCode`.
- **Passwörter:**
  - Sie liegen nur versiegelt in der Datenbank, mit der Vault-Chiffre und der AAD `password:<id>`.
  - Sie stehen nie in einem Log und nie in einer Fehlermeldung.
  - Kein Tauri-Befehl gibt ein gespeichertes Passwort an die Oberfläche zurück. Die Oberfläche erfährt nur, ob eines gesetzt ist.
  - Im Speicher liegen sie als `Zeroizing<String>`.
- **Jeder Hop nur mit seinen eigenen Zugangsdaten:** Ein Sprung-Host bekommt nie Passwort oder Schlüssel des Ziels, das Ziel nie die des Sprung-Hosts. Ein Proxy bekommt nur seine eigenen Zugangsdaten.
- **Ein gespeichertes Passwort beantwortet bei keyboard-interactive nur eine Runde mit genau einer verdeckten Frage (`echo == false`).** Alles andere fragt den Benutzer.
- **Agent-Weiterleitung** ist standardmäßig aus.
  - Eingeschaltet gilt sie nur für das Ziel, nie für Sprung-Hosts.
  - Ein Agent-Kanal, den der Server ohne eingeschaltete Weiterleitung öffnet, wird abgelehnt.
  - Weitergeleitet wird nur der System-Agent (Proton Pass), nie Schlüssel aus dem SSHCraft-Tresor.
- **Grenzen:**
  - Höchstens 8 Sprung-Hosts (`MAX_JUMPS`).
  - Höchstens 64 Umgebungsvariablen je Ebene (`MAX_ENV`). Namen nach `^[A-Za-z_][A-Za-z0-9_]{0,63}$`, Werte höchstens 4096 Bytes.
  - TCP- und Proxy-Aufbau je Hop höchstens 15 s (`CONNECT_TIMEOUT`).
- Seriell und lokales Terminal gibt es nur auf dem Desktop (`cfg(desktop)` wie bei den bisherigen Desktop-Abhängigkeiten). Auf dem Handy liefern die Listenbefehle leere Listen, das Öffnen liefert `ErrorCode::NotSupported`.

## Review Focus

1. **Ein gespeichertes Passwort geht an die falsche Stelle:** an eine sichtbare keyboard-interactive-Frage, an einen Sprung-Host, an den Proxy oder an das Ziel, obwohl es dem Sprung-Host gehört. Tests in Task 5 und 6.
2. **Ein Server öffnet einen Agent-Kanal, ohne dass die Weiterleitung an ist, oder ein Sprung-Host versucht es:** Der Kanal wird abgelehnt, und der Agent bekommt keine Anfrage. Test in Task 7.
3. **Kaputte Sprung-Ketten:** eine Kette mit dem Ziel selbst, ein Host doppelt, ein gelöschter Host, mehr als 8 Sprünge oder ein Sprung-Host ohne Adresse. Erwartet wird ein klarer Fehler vor jedem Netzverkehr und kein endloses Verbinden. Test in Task 1.
4. **Passwörter im Klartext:** in der Datenbankdatei, im Sync-Datensatz außerhalb der Ende-zu-Ende-Verschlüsselung, in einem Rückgabewert an die Oberfläche oder in einem Log. Tests in Task 2, 3 und 8.
5. **Freigabe ohne Schlüssel:** Passwörter (auch die einer Identität oder eines Proxys) erreichen keinen Empfänger. Mit Schlüsseln erreichen sie ihn, und ein Empfänger kann sie nicht ändern. Tests in Task 4.

---

## Dateistruktur

```
crates/sshcraft-core/src/model.rs          Identity, Proxy, EnvVar, neue ConnectionSettings, resolve_route
crates/sshcraft-core/src/store.rs          Schema 6: identities, passwords; Orte tragen Passwörter mit
crates/sshcraft-core/src/sync_store.rs     ItemKind::Identity, ItemKind::Password
crates/sshcraft-core/src/vault.rs          Passwörter versiegeln und öffnen
crates/sshcraft-core/src/policy.rs         ShellEvent (von sshcraft-ssh hierher)
crates/sshcraft-sync/src/{items,engine,shares}.rs   Identitäten und Passwörter im Sync und beim Teilen
crates/sshcraft-ssh/src/lib.rs             Auth, Route, Hops, SessionOptions
crates/sshcraft-ssh/src/auth.rs            Passwort, keyboard-interactive, AuthPrompter
crates/sshcraft-ssh/src/proxy.rs           HTTP CONNECT, SOCKS5
crates/sshcraft-ssh/src/forward.rs         Agent-Weiterleitung
crates/sshcraft-ssh/src/testing.rs         Testserver mit Passwort, kbd-interactive, direct-tcpip, Env, Agent; Testproxys
crates/sshcraft-local/                     neu: pty.rs (lokales Terminal), serial.rs
app/src-tauri/src/sessions.rs              SessionTarget, Route aus dem Store, Prompts für die Anmeldung
app/src-tauri/src/prompts.rs               Anmelde-Fragen neben den Host-Schlüssel-Fragen
app/src-tauri/src/commands/{identities,passwords,local}.rs  neu
app/src/pages/KeysPage.tsx                 Abschnitt Identitäten
app/src/components/{HostForm,GroupForm,ConnectionFields,AuthPromptDialog,IdentityForm}.tsx
app/src/pages/NewTabPage.tsx               Lokales Terminal, Seriell
docs/connections.md                        neu
```

---

### Task 1: Datenmodell und Auflösung der Route

**Files:**
- Modify: `crates/sshcraft-core/src/model.rs`
- Test: Modelltests in `model.rs`, neue Datei `crates/sshcraft-core/src/route_tests.rs` (per `#[path]`)

**Interfaces:**
- Produces:
  - `id_type!(IdentityId)`, `id_type!(PasswordId)`.
  - `ConnectionSettings` bekommt neue Felder, alle `#[serde(default)]`, damit alte Datensätze lesbar bleiben:
    - `password: Option<PasswordId>`
    - `identity: Option<IdentityId>`
    - `jump_hosts: Option<Vec<HostId>>`: `None` erbt, `Some(vec![])` heißt ausdrücklich ohne.
    - `proxy: Option<Proxy>`
    - `agent_forwarding: Option<bool>`
    - `env: Vec<EnvVar>`
  - `#[serde(tag = "kind", rename_all = "snake_case")] enum Proxy { Direct, Http(ProxyEndpoint), Socks5(ProxyEndpoint) }`. `Direct` beendet die Vererbung.
  - `struct ProxyEndpoint { address: String, port: u16, username: Option<String>, password: Option<PasswordId> }`.
  - `struct EnvVar { name: String, value: String }`.
  - `struct Identity { id: IdentityId, name: String, username: Option<String>, key: Option<KeyRef>, password: Option<PasswordId>, #[serde(default, skip_serializing_if = "Option::is_none")] location: Option<ServerId> }`.
  - `normalize_env(&[EnvVar]) -> Vec<EnvVar>`:
    - Ungültige Namen und zu lange Werte fallen weg.
    - Gleiche Namen: der letzte gewinnt, an der Stelle des ersten.
    - Höchstens `MAX_ENV = 64`.
  - `ResolvedHost` ändert sich:
    - `key: Option<KeyRef>`
    - neu: `password: Option<PasswordId>`, `proxy: Option<Proxy>` (nie `Direct`, das wird zu `None`), `agent_forwarding: bool`, `env: Vec<EnvVar>`, `jump_hosts: Vec<HostId>`.
  - `ResolveError::MissingKey` fällt weg. Ein Host ohne Schlüssel und ohne Passwort ist gültig, nach dem Passwort wird beim Verbinden gefragt. `ErrorCode::HostMissingKey` und `errors.host_missing_key` (en, de) fallen mit weg.
  - `resolve(host: &Host, chain: &[&Group], identities: &[Identity]) -> Result<ResolvedHost, ResolveError>`. Die Regel steht in Step 3.
  - `pub struct Directory<'a> { pub groups: &'a [Group], pub hosts: &'a [Host], pub identities: &'a [Identity] }`.
  - `pub struct Route { pub target: ResolvedHost, pub jumps: Vec<ResolvedHost> }`.
  - `pub fn resolve_route(dir: &Directory, host_id: HostId) -> Result<Route, ResolveError>`:
    - Die Sprung-Hosts werden mit ihren eigenen Werten aufgelöst.
    - Ihre eigenen `jump_hosts` und ihr eigener `proxy` gelten dabei nicht. Es zählt allein, was am Ziel steht.
  - `MAX_JUMPS = 8`.
  - Neue Fehler:
    - `ResolveError::NotFound`
    - `JumpLoop` (Ziel in der Kette oder ein Host doppelt)
    - `TooManyJumps`
    - `JumpMissing` (Host existiert nicht)
    - `Jump { host_id: HostId, error: Box<ResolveError> }` (ein Sprung-Host lässt sich nicht auflösen)

- [ ] **Step 1: Tests schreiben.**
  - `identity_fills_what_the_level_leaves_open`: Host mit Identität (Benutzer `deploy`, Schlüssel K) und eigenem Benutzer `root`. Ergebnis: Benutzer `root`, Schlüssel K.
  - `host_identity_beats_group_value`: Gruppe setzt Benutzer `a`, Host-Identität setzt `b`. Ergebnis: `b`.
  - `group_identity_inherits_down`.
  - `missing_identity_is_ignored`.
  - `no_key_no_password_resolves`.
  - `proxy_direct_stops_inheritance`.
  - `jump_hosts_empty_list_stops_inheritance`.
  - `env_merges_top_down_host_wins`: Gruppe `A=1,B=1`, Host `B=2`. Ergebnis: `A=1,B=2`.
  - `normalize_env_drops_bad_names`: `1X`, `A-B`, leerer Name, ein Wert mit 4097 Bytes.
  - `route_resolves_jumps_with_their_own_credentials`: Der Sprung-Host hat Passwort P1, das Ziel P2. In der Route trägt `jumps[0].password` P1 und `target.password` P2.
  - `route_ignores_jump_hosts_own_proxy_and_jumps`.
  - `route_rejects_target_in_chain` (`JumpLoop`).
  - `route_rejects_duplicate_hop` (`JumpLoop`).
  - `route_rejects_nine_jumps` (`TooManyJumps`).
  - `route_reports_deleted_jump` (`JumpMissing`).
  - `route_reports_jump_without_address` (`Jump { .. MissingAddress }`).
  - `old_settings_json_reads`: JSON ohne die neuen Felder.
- [ ] **Step 2:** `cargo test -p sshcraft-core` muss rot sein.
- [ ] **Step 3: Umsetzen.** Je Ebene (erst der Host, dann die Gruppen der Kette) gilt ein Wert der Ebene selbst. Fehlt er dort, gilt der Wert der Identität dieser Ebene (nur `username`, `key`, `password`). Erst dann kommt die nächste Ebene. `env` wird von oben nach unten zusammengelegt. Alle Aufrufer von `resolve` umstellen: `app/src-tauri/src/sessions.rs::lookup` vorerst auf `resolve_route(...).target`, dazu `commands/data.rs`, `export.rs` und die Importe.
- [ ] **Step 4:** `cargo test --workspace` muss grün sein, `cargo clippy --workspace --all-targets -- -D warnings` sauber.
- [ ] **Step 5: Gegenprobe.** In `resolve_route` die Doppelprüfung entfernen. `route_rejects_duplicate_hop` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: Identitäten, Passwortverweis, Sprung-Hosts, Proxy und Umgebung im Modell`

---

### Task 2: Store, Tresor und Orte

**Files:**
- Modify: `crates/sshcraft-core/src/store.rs`, `src/sync_store.rs`, `src/vault.rs`
- Test: `crates/sshcraft-core/src/store_tests.rs`, `src/vault_tests.rs`

**Interfaces:**
- Consumes: Typen aus Task 1.
- Produces:
  - Schema 6, mit Versionsschritt wie bei 5:
    - `CREATE TABLE identities (id TEXT PRIMARY KEY, name TEXT NOT NULL, body TEXT NOT NULL, location TEXT)`. `body` hält `username`, `key` und `password` als JSON.
    - `CREATE TABLE passwords (id TEXT PRIMARY KEY, sealed BLOB NOT NULL, location TEXT)`.
  - `ItemKind::Identity` (`"identity"`) und `ItemKind::Password` (`"password"`). `location_of`, `set_location`, `read_only`, `received_share_of` und alle `match` über `ItemKind` kennen sie.
  - `Store::upsert_identity(&Identity)`, `list_identities() -> Vec<Identity>`, `delete_identity(IdentityId)`.
  - `put_password_record(id, sealed: &[u8], location)`, `password_record(id) -> Option<(Vec<u8>, Option<ServerId>)>`, `delete_password(id)`, `list_password_ids() -> Vec<PasswordId>`.
  - `Vault::store_password(&self, store, id: PasswordId, secret: &str, location: Option<ServerId>) -> Result<(), VaultError>`.
  - `Vault::password(&self, store, id) -> Result<Zeroizing<String>, VaultError>` (`NotFound`, `Corrupt`).
  - AAD `password:<uuid>`, so ein versiegeltes Passwort nicht als Schlüssel oder anderes Geheimnis durchgeht.
  - Passwörter folgen ihrem Besitzer. Trägt `set_location` einen Host, eine Gruppe (samt Untergruppen und Hosts) oder eine Identität an einen anderen Ort, gehen die Passwörter mit, auf die deren `settings.password`, `defaults.password`, `identity.password` und `proxy.password` zeigen.
  - Gleiches gilt für `upsert_host` und `upsert_group`, wenn sich dabei der Ort ändert.
  - `normalize_env` läuft in `upsert_host` und `upsert_group` wie `normalize_tags`.

- [ ] **Step 1: Tests schreiben.**
  - `migration_from_v5_keeps_data`.
  - `identity_roundtrip`.
  - `password_roundtrip`.
  - `password_is_sealed_on_disk`: Die Datenbankdatei als Bytes lesen. Das Passwort `hunter2-sshcraft` kommt darin nicht vor.
  - `password_aad_binds_id`: Ein versiegeltes Passwort unter einer anderen Id liefert `Corrupt`.
  - `password_cannot_be_opened_as_key`.
  - `moving_a_host_moves_its_password`.
  - `moving_a_group_moves_passwords_below_it` (auch das Proxy-Passwort eines Hosts darunter).
  - `env_is_normalized_on_save`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-core` grün und clippy sauber.
- [ ] **Step 5: Gegenprobe.** In `set_location` das Mitnehmen der Passwörter entfernen. `moving_a_host_moves_its_password` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: Identitäten und Passwörter im Store und im Tresor (Schema 6)`

---

### Task 3: Identitäten und Passwörter im Sync

**Files:**
- Modify: `crates/sshcraft-sync/src/items.rs`, `src/engine.rs`
- Test: `crates/sshcraft-sync/tests/push.rs`, `tests/pull.rs`, neue Datei `tests/credentials.rs`

**Interfaces:**
- Consumes: Store und Tresor aus Task 2.
- Produces:
  - `ItemBody::Identity { identity: Identity }` (ohne `location`) und `ItemBody::Password { id: PasswordId, secret: Zeroizing<String> }`. `SyncItem::item`, `local_item`, `apply_in` und der Löschfall kennen beide.
  - **Regel: Passwörter laufen überall wie Schlüssel.**
    - Ist `sync_keys` aus, werden sie nicht gesendet und nicht angenommen.
    - In der Outbox stehen sie zusammen mit den Schlüsseln vorn (Rang 0).
    - Identitäten haben Rang 1, dann kommen die Gruppen nach Tiefe, dann die Hosts.
  - Die Stellen in `engine.rs`, die heute `ItemKind::Key` besonders behandeln, behandeln `ItemKind::Password` genauso: Outbox-Rang, `sync_keys`, Empfang, Löschen, `is_key_record`.
  - Die Funktion, die beim Senden fremde Schlüsselverweise entfernt (heute für `settings.key` und `defaults.key`), gilt auch für die neuen Verweise:
    - `settings.password`, `defaults.password`, `identity.password` und `proxy.password`: Gesendet wird ein Verweis nur, wenn das Passwort am selben Server im selben Bereich liegt.
    - `settings.identity`, `defaults.identity`: dieselbe Regel.
    - `identity.key`: wie heute `settings.key`.
  - `apply_in` behält einen lokalen Verweis, wenn der Datensatz keinen trägt, nach derselben Regel wie `local_key`. Die Funktion wird verallgemeinert, damit es keine drei Kopien gibt.
  - Das Setting heißt weiter `sync_keys`, die Oberfläche zeigt "Schlüssel und Passwörter synchronisieren" (Task 12).

- [ ] **Step 1: Tests schreiben.**
  - `identity_and_password_reach_second_device`: Gerät A legt eine Identität mit Passwort an und verbindet den Host damit. Gerät B löst nach dem Pull dieselbe Route auf, mit Benutzer und Passwort.
  - `password_not_sent_without_sync_keys`: Auf dem Server liegt danach kein Datensatz mit `ItemBody::Password`, festgestellt über die Zahl der Datensätze und das Entschlüsseln auf B.
  - `server_never_sees_password_plaintext`: Alle Datensätze auf dem Testserver als Bytes enthalten `hunter2-sshcraft` nicht.
  - `password_at_other_location_is_not_referenced`: Host am Server S, Passwort nur auf dem Gerät. Der Datensatz trägt `password: None`, und Gerät A behält seinen Verweis nach dem eigenen Pull.
  - `deleting_identity_reaches_second_device`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-sync` grün.
- [ ] **Step 5: Gegenprobe.** Die `sync_keys`-Prüfung für `ItemKind::Password` beim Senden entfernen. `password_not_sent_without_sync_keys` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-sync: Identitäten und Passwörter synchronisieren`

---

### Task 4: Identitäten und Passwörter beim Teilen

**Files:**
- Modify: `crates/sshcraft-sync/src/shares.rs`, `src/engine.rs`
- Test: `crates/sshcraft-sync/tests/shares.rs`, `tests/share_rights.rs`

**Interfaces:**
- Consumes: Task 3.
- Produces: **Regel: Was heute für Schlüssel einer Freigabe gilt, gilt für Passwörter genauso. Identitäten gehen immer mit.**
  - Die Sammelfunktion (heute `(keys, agent)` aus den geteilten Hosts und Gruppen) wird zu `struct Credentials { keys: Vec<KeyId>, passwords: Vec<PasswordId>, identities: Vec<IdentityId>, agent_keys_skipped: bool }`.
  - Sie sammelt aus Host, Gruppen, deren Identitäten und Proxys. Ausgenommen ist, was aus fremden Freigaben stammt (wie heute bei Schlüsseln).
  - **Gruppe mit allen Hosts:**
    - Identitäten wandern immer in den Bereich (`set_item_area`), denn ohne sie fehlt dem Empfänger der Benutzer.
    - Schlüssel und Passwörter wandern nur mit `with_keys`.
    - Ohne `with_keys` entfernt die Senderegel aus Task 3 die Schlüssel- und Passwortverweise der Identität im Datensatz.
  - **Auswahl und einzelner Host:**
    - Identitäten werden kopiert (`copy_id`), ihre Verweise auf Kopien übersetzt (wie heute `KeyRef` auf die Schlüsselkopie).
    - Passwörter werden mit `with_keys` kopiert, sonst entfernt.
    - Reihenfolge der Kopien: Schlüssel, Passwörter, Identitäten, Gruppe, Hosts.
  - **Rechte:** Ein Empfänger darf Schlüssel, Passwörter und Identitäten einer Freigabe nicht ändern, auch mit Schreibrecht nicht. Die Prüfung `kind == ItemKind::Key` wird auf alle drei erweitert (`share_read_only`).
  - Eine Kopie folgt ihrem Original, wenn der Besitzer das Passwort oder die Identität ändert (wie heute bei Schlüsselkopien).

- [ ] **Step 1: Tests schreiben.**
  - `group_share_with_keys_carries_password_and_identity`: Der Empfänger löst die Route mit Benutzer aus der Identität und dem Passwort auf.
  - `group_share_without_keys_carries_identity_user_only`: Der Empfänger hat den Benutzer, aber kein Passwort. Auf dem Server liegt kein Passwort-Datensatz im Bereich.
  - `selection_share_copies_identity_and_password`.
  - `proxy_password_follows_with_keys`.
  - `member_cannot_change_shared_password` (`share_read_only`).
  - `owner_password_change_reaches_member`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-sync` grün.
- [ ] **Step 5: Gegenprobe.** Bei einer Gruppenfreigabe die Passwörter ohne Rücksicht auf `with_keys` in den Bereich legen. `group_share_without_keys_carries_identity_user_only` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-sync: Passwörter und Identitäten beim Teilen`

---

### Task 5: Anmeldung mit Passwort und keyboard-interactive

**Files:**
- Create: `crates/sshcraft-ssh/src/auth.rs`
- Modify: `crates/sshcraft-ssh/src/lib.rs`, `src/testing.rs`
- Test: `crates/sshcraft-ssh/tests/auth.rs`

**Interfaces:**
- Produces:
  - `pub struct Auth { pub key: Option<Credential>, pub password: Option<Zeroizing<String>> }`. `Credential` bleibt `Key | Agent`.
  - `pub struct AuthRequest { pub host_name: String, pub username: String, pub name: String, pub instructions: String, pub prompts: Vec<AuthPromptLine> }` und `pub struct AuthPromptLine { pub text: String, pub echo: bool }`.
  - `#[async_trait] pub trait AuthPrompter: Send + Sync { async fn answer(&self, request: AuthRequest) -> Option<Vec<Zeroizing<String>>>; }`. `None` heißt abgebrochen.
  - `pub(crate) async fn authenticate(handle, username, host_name, auth: Auth, prompter: &dyn AuthPrompter) -> Result<(), SshError>`. Die Reihenfolge steht in Step 3.
  - Neue Fehler: `SshError::AuthCancelled` (wird zu `Disconnect::AuthFailed`) und `SshError::PasswordRejected`.
  - Testserver:
    - `TestServer::start_with(TestAuth)` mit `TestAuth { key: Option<PublicKey>, password: Option<String>, kbd: Vec<Vec<(String, bool, String)>>, key_then_kbd: bool }`. `kbd` ist eine Liste von Runden aus Frage, `echo` und erwarteter Antwort. `key_then_kbd` bedeutet Teilerfolg mit Schlüssel, danach kbd.
    - `TestServer::received_passwords() -> Vec<String>` sammelt alle Passwort- und kbd-Antworten in der Reihenfolge des Eingangs.
    - `start(authorized)` bleibt als Kurzform.

- [ ] **Step 1: Tests schreiben.** Ein `RecordingPrompter` zeichnet Anfragen auf und antwortet aus einer Liste.
  - `password_auth_succeeds`.
  - `wrong_password_is_rejected`: `PasswordRejected`, der Prompter wird nicht gefragt.
  - `stored_password_answers_single_hidden_kbd_prompt`: Der Server bietet nur kbd mit einer Runde `("Password: ", false)`. Erfolg, ohne den Prompter zu fragen.
  - `visible_kbd_prompt_is_never_answered_with_stored_password`: Eine Runde `("Username: ", true)`. Der Prompter wird gefragt, `received_passwords()` enthält das gespeicherte Passwort nicht.
  - `two_prompt_round_asks_user`: Passwort plus Code in einer Runde. Der Prompter bekommt beide Zeilen.
  - `key_then_otp_partial_success`.
  - `cancel_gives_auth_cancelled`.
  - `no_stored_password_asks_once_for_password_method`: Der Server kann nur `password`. Der Prompter bekommt eine verdeckte Zeile.
  - `key_auth_unchanged`: Der bisherige Agent- und Schlüsselweg ist grün.
- [ ] **Step 2:** `cargo test -p sshcraft-ssh` muss rot sein.
- [ ] **Step 3: Umsetzen.** Reihenfolge der Anmeldung:
  1. Mit einem Schlüssel zuerst `publickey`.
  2. Dann mit einem gespeicherten Passwort `password`, wenn der Server es anbietet.
  3. Dann `keyboard-interactive`, wenn angeboten. Das gespeicherte Passwort beantwortet höchstens eine Runde und nur, wenn sie genau eine Zeile mit `echo == false` hat und das Passwort noch nicht abgelehnt wurde. Jede andere Runde geht an den Prompter.
  4. Ohne gespeichertes Passwort, wenn der Server nur `password` anbietet: einmal den Prompter mit einer verdeckten Zeile fragen.

  Die angebotenen Methoden kommen aus `remaining_methods` der Fehlantworten, `partial_success` setzt fort. Höchstens 3 kbd-Runden mit Benutzerfragen. Antworten liegen nur als `Zeroizing<String>` vor.
- [ ] **Step 4:** `cargo test -p sshcraft-ssh` grün, clippy sauber.
- [ ] **Step 5: Gegenprobe.** Die Prüfung `echo == false` entfernen. `visible_kbd_prompt_is_never_answered_with_stored_password` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-ssh: Anmeldung mit Passwort und keyboard-interactive`

---

### Task 6: Route mit Proxy und Sprung-Hosts

**Files:**
- Create: `crates/sshcraft-ssh/src/proxy.rs`
- Modify: `crates/sshcraft-ssh/src/lib.rs`, `src/testing.rs`, `crates/sshcraft-ssh/Cargo.toml` (`tokio-socks`, `data-encoding`)
- Test: `crates/sshcraft-ssh/tests/route.rs`

**Interfaces:**
- Consumes: `Auth`, `AuthPrompter` aus Task 5.
- Produces:
  - `pub struct Hop { pub host_name: String, pub address: String, pub port: u16, pub username: String, pub auth: Auth }`.
  - `pub enum ProxyConfig { Http(ProxyTarget), Socks5(ProxyTarget) }` mit `pub struct ProxyTarget { pub address: String, pub port: u16, pub username: Option<String>, pub password: Option<Zeroizing<String>> }`.
  - `pub struct RouteParams { pub hops: Vec<Hop>, pub proxy: Option<ProxyConfig>, pub options: SessionOptions }`. `hops` hält erst die Sprung-Hosts, zuletzt das Ziel, mindestens ein Eintrag.
  - `SessionOptions` legt dieser Task als leere `#[derive(Default)] pub struct SessionOptions {}` an, Task 7 füllt sie.
  - `pub async fn connect_route(params: RouteParams, verifier: Arc<dyn HostKeyVerifier>, prompter: Arc<dyn AuthPrompter>) -> Result<Session, SshError>`. `connect(ConnectParams, …)` bleibt als Kurzform für einen Hop ohne Proxy (bestehende Tests).
  - `HostKeyVerifier::verify(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool`. Der Name kommt dazu, damit die Frage den richtigen Hop nennt.
  - `Session` hält die Handles der Sprung-Hosts, bis die Sitzung endet.
  - Neue Fehler:
    - `SshError::Proxy(String)`
    - `SshError::ProxyAuthFailed`
    - `SshError::Hop { index: usize, host_name: String, error: Box<SshError> }`. Gilt für Fehler an Sprung-Hosts. Fehler am Ziel bleiben ohne Hülle. `From<&SshError> for Disconnect` sieht durch die Hülle hindurch.
  - `pub fn client_config() -> client::Config`: Keep-Alive wie bisher, `preferred` bleibt `Preferred::DEFAULT`.
  - HTTP CONNECT:
    - Anfrage `CONNECT host:port HTTP/1.1`, `Host:`, mit Zugangsdaten `Proxy-Authorization: Basic …`.
    - Antwort bis `\r\n\r\n`, höchstens 8 KiB.
    - `200` ist Erfolg, `407` gibt `ProxyAuthFailed`, alles andere `Proxy("HTTP <code>")`.
  - SOCKS5 über `tokio_socks::tcp::Socks5Stream::connect` und `connect_with_password`. Den Zielnamen löst der Proxy auf. Ein Auth-Fehler gibt `ProxyAuthFailed`.
  - Testhelfer in `testing.rs`:
    - `spawn_http_proxy(credentials: Option<(String, String)>) -> TestProxy`
    - `spawn_socks5_proxy(credentials: Option<(String, String)>) -> TestProxy`
    - `TestProxy { port, connects: Arc<AtomicUsize>, targets: Arc<Mutex<Vec<String>>> }`
    - Der Testserver nimmt `direct-tcpip` an, verbindet zu `127.0.0.1:<port>` und leitet weiter.

- [ ] **Step 1: Tests schreiben.**
  - `kex_prefers_mlkem`: `client_config().preferred.kex[0] == russh::kex::MLKEM768X25519_SHA256`.
  - `through_http_proxy`.
  - `http_proxy_wrong_password` (`ProxyAuthFailed`).
  - `through_socks5_proxy_with_password`.
  - `socks5_proxy_resolves_name`: Das Ziel heißt `localhost`, beim Proxy kommt der Name an, nicht die IP.
  - `through_one_jump_host`.
  - `through_two_jump_hosts`.
  - `jump_host_key_is_verified_with_its_own_name`: Der Verifier sieht beide Adressen und Namen in der Reihenfolge Sprung, Ziel.
  - `each_hop_gets_only_its_own_password`: Sprung-Host mit Passwort A, Ziel mit B. `received_passwords()` ist beim Sprung-Server `[A]` und beim Ziel `[B]`.
  - `jump_auth_failure_names_the_hop` (`Hop { index: 0, .. }`).
  - `proxy_is_used_for_the_first_hop_only`: Der Proxy zählt genau eine Verbindung, mit dem Sprung-Host als Ziel.
  - `unreachable_proxy_times_out`: Port ohne Listener, oder ein Listener, der nie antwortet, mit einem für den Test verkürzten Timeout über `#[cfg(test)]`-Konstante oder Parameter.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-ssh` grün und clippy sauber. Den Aufrufer in `app/src-tauri/src/sessions.rs` auf die neue Signatur von `verify` umstellen.
- [ ] **Step 5: Gegenprobe.** Beim Sprung-Host statt `hop.auth` die `Auth` des Ziels verwenden. `each_hop_gets_only_its_own_password` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-ssh: Sprung-Hosts, HTTP- und SOCKS5-Proxy`

---

### Task 7: Agent-Weiterleitung und Umgebungsvariablen

**Files:**
- Create: `crates/sshcraft-ssh/src/forward.rs`
- Modify: `crates/sshcraft-ssh/src/lib.rs`, `src/agent.rs`, `src/testing.rs`
- Test: `crates/sshcraft-ssh/tests/forward.rs`

**Interfaces:**
- Consumes: `RouteParams` aus Task 6.
- Produces:
  - `pub trait AgentStream: AsyncRead + AsyncWrite + Send + Unpin {}` mit Blanket-Impl.
  - `#[async_trait] pub trait AgentOpener: Send + Sync { async fn open(&self) -> Result<Box<dyn AgentStream>, SshError>; }`.
  - Unter Windows `agent::WindowsAgentOpener`: öffnet `\\.\pipe\openssh-ssh-agent` über `tokio::net::windows::named_pipe::ClientOptions`, mit 5 s Timeout.
  - `#[derive(Default)] pub struct SessionOptions { pub agent_forwarding: Option<Arc<dyn AgentOpener>>, pub env: Vec<(String, String)> }`.
  - `open_shell` sendet vor `request_shell` je Variable `set_env(false, name, value)` und mit Weiterleitung `agent_forward(false)`.
  - Der Client-Handler nimmt `server_channel_open_agent_forward` nur an, wenn dieser Hop das Ziel ist und `agent_forwarding` gesetzt ist. Dann öffnet er den Agent und kopiert in beide Richtungen (`copy_bidirectional` mit `channel.into_stream()`). Sonst lehnt er ab.
  - Testserver:
    - zeichnet `env_request` auf (`received_env() -> Vec<(String, String)>`)
    - öffnet nach `shell_request` einen Agent-Kanal zum Client (immer, auch ungefragt)
    - fragt darüber `request_identities` und legt das Ergebnis ab: `forwarded_keys() -> Option<usize>`. `None` heißt, der Kanal wurde abgelehnt.
  - `testing::agent_opener(keys: &[PrivateKey]) -> Arc<dyn AgentOpener>` mit dem Test-Agent über `tokio::io::duplex`.

- [ ] **Step 1: Tests schreiben.**
  - `env_vars_reach_server`.
  - `forwarded_agent_lists_keys` (`Some(1)`).
  - `agent_channel_rejected_without_forwarding` (`None`).
  - `jump_host_cannot_open_agent_channel`: Weiterleitung an. Der Sprung-Server versucht es und bekommt `None`, das Ziel bekommt `Some(1)`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-ssh` grün.
- [ ] **Step 5: Gegenprobe.** Im Handler immer annehmen. `agent_channel_rejected_without_forwarding` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-ssh: Agent-Weiterleitung und Umgebungsvariablen`

---

### Task 8: Sitzungen in der App verbinden über die Route

**Files:**
- Modify: `app/src-tauri/src/sessions.rs`, `src/prompts.rs`, `src/errors.rs`, `src/lib.rs`, `src/commands/sessions.rs`
- Create: `app/src-tauri/src/commands/identities.rs`, `src/commands/passwords.rs`
- Test: `app/src-tauri/src/sessions_tests.rs`, Tests in `prompts.rs`

**Interfaces:**
- Consumes: Tasks 1 bis 7.
- Produces:
  - `connect_once` baut die Route aus dem Store:
    - Auflösung mit `resolve_route`.
    - Je Hop werden Schlüssel und Passwort geladen. Vor dem ersten Geheimnis aus dem Tresor, auch einem Passwort, kommt einmal das `KeyGate` (Windows Hello).
    - Das Proxy-Passwort wird geladen.
    - Für die Weiterleitung `WindowsAgentOpener`, unter Nicht-Windows ohne Weiterleitung, mit einem Hinweis im Status.
    - `env` aus dem `ResolvedHost`.
    - Fehlt ein Passwort-Eintrag auf dem Gerät, wird es behandelt wie kein Passwort; gefragt wird dann beim Verbinden.
  - `PromptRegistry` stellt neben Host-Schlüssel-Fragen auch Anmeldefragen:
    - Struktur `AuthPrompt { prompt_id, host_name, username, name, instructions, prompts: Vec<{ text, echo }> }` (`Serialize`, ohne Antworten).
    - Event `auth-prompt`, Befehl `answer_auth(prompt_id: u64, answers: Option<Vec<String>>)`. Die Antworten wandern sofort in `Zeroizing<String>`.
    - Das Zurückziehen beim Schließen eines Tabs gilt für beide Fragearten.
  - Beim Wiederverbinden wird neu gefragt. Antworten des Benutzers werden nicht gemerkt.
  - Neuer Endgrund `EndReason::AuthCancelled`: kein Wiederverbinden.
  - Neue `ErrorCode`s mit Texten (en, de):
    - `ProxyFailed`, `ProxyAuthFailed`
    - `JumpFailed`, mit `detail` = Name des Hops, auch für `Hop`-Fehler beim Anmelden
    - `JumpLoop`, `TooManyJumps`, `JumpMissing`
    - `PasswordRejected`
    - `NotSupported`
  - Befehle:
    - `identities_list() -> Vec<IdentityView>` (`IdentityView` = `Identity` plus `has_password: bool`, ohne Passwort-Id)
    - `identity_save(identity: IdentityInput) -> IdentityView` mit `IdentityInput { id: Option<IdentityId>, name, username, key: Option<KeyRef>, location: Option<ServerId>, password: PasswordChange }`
    - `identity_delete(id)`: löscht auch das eigene Passwort, Verweise darauf werden bei der Auflösung übergangen
    - `password_set(owner: PasswordOwner, secret: String) -> ()` und `password_clear(owner: PasswordOwner) -> ()`. `PasswordOwner = Host { id } | Group { id } | HostProxy { id } | GroupProxy { id } | Identity { id }`. Der Befehl legt das Passwort unter neuer oder bestehender Id an oder löscht es, setzt den Verweis im Besitzer und meldet die Änderungen an den Sync (`record_local_change` für Passwort und Besitzer).
    - `PasswordChange = Keep | Set { secret } | Clear`.
  - Kein Befehl gibt ein Passwort zurück. Die Ansichten für Host und Gruppe tragen `has_password` und `proxy_has_password`; Ids von Passwörtern kommen nicht in die Oberfläche.
  - Rechte wie bei Hosts: In einer Freigabe mit Leserecht verweigern `password_set` und `password_clear` (`ShareReadOnly`), ebenso für einen Empfänger beim Passwort einer Freigabe mit Schlüsseln (Task 4).

- [ ] **Step 1: Tests schreiben.**
  - `password_host_connects`: Der Store hat Host und Passwort, der Testserver will das Passwort.
  - `jump_route_connects`.
  - `kbd_prompt_reaches_registry_and_answer_logs_in`.
  - `closing_tab_withdraws_auth_prompt`.
  - `cancel_ends_without_reconnect` (`AuthCancelled`).
  - `vault_locked_with_password_reports_vault_locked`.
  - `hello_asked_before_password_use`: Ein Presence-Fake zählt einen Aufruf.
  - `password_set_and_clear_roundtrip`.
  - `password_never_returned`: Das JSON von `identities_list` und der Host-Ansicht enthält das Passwort nicht, auch nicht seine Id.
  - `read_only_share_refuses_password_set`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-app` grün und clippy sauber.
- [ ] **Step 5: Gegenprobe.** In `connect_once` das `KeyGate` beim Passwort überspringen. `hello_asked_before_password_use` muss rot werden.
- [ ] **Step 6: Commit** `App: Verbindungen über Route, Passwörter, Identitäten und Anmeldefragen`

---

### Task 9: Crate `sshcraft-local`: lokales Terminal

**Files:**
- Create: `crates/sshcraft-local/{Cargo.toml,src/lib.rs,src/pty.rs}`
- Modify: `Cargo.toml` (Workspace-Mitglied, `portable-pty`, `serialport`), `crates/sshcraft-core/src/policy.rs` (neue Heimat von `ShellEvent`), `crates/sshcraft-ssh/src/lib.rs` (`pub use sshcraft_core::policy::ShellEvent;`)
- Test: `crates/sshcraft-local/tests/pty.rs`, Unit-Tests in `pty.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, Clone)] pub struct ShellCommand { pub program: String, pub args: Vec<String>, pub label: String }`.
  - `pub fn available_shells(custom: Option<ShellCommand>) -> Vec<ShellCommand>`:
    - Windows: `powershell.exe` immer, `pwsh.exe` wenn im `PATH`, `cmd.exe`, je WSL-Distribution `wsl.exe -d <name>`, dann `custom`.
    - Sonst: `$SHELL -l`, Rückfall `/bin/sh`, dann `custom`.
  - `pub fn parse_wsl_list(bytes: &[u8]) -> Vec<String>`: Die Ausgabe von `wsl.exe --list --quiet` ist UTF-16LE, eventuell mit BOM. Leere Zeilen fallen weg, Namen werden getrimmt.
  - `pub fn spawn(cmd: &ShellCommand, cols: u16, rows: u16) -> Result<(LocalWriter, mpsc::Receiver<ShellEvent>), LocalError>`:
    - Startet im Benutzerverzeichnis.
    - Setzt `TERM=xterm-256color`.
    - Ein Lesefaden schickt `ShellEvent::Data`, nach dem Ende des Kindprozesses `Ended(Disconnect::Exited(code))`.
  - `LocalWriter::write(&self, &[u8])`, `resize(cols, rows)`. Er ist `Clone` und `Send`.
  - Beim Drop des letzten Writers wird der Kindprozess beendet.
  - `LocalError { Spawn(String), Io(String) }`.

- [ ] **Step 1: Tests schreiben.**
  - `parse_wsl_list_utf16_with_bom`: `"Ubuntu\r\n\r\nDebian\r\n"` als UTF-16LE mit BOM ergibt `["Ubuntu", "Debian"]`.
  - `parse_wsl_list_garbage_is_empty`: ungerade Byte-Zahl.
  - `spawned_shell_echoes`: Windows `cmd.exe /c echo sshcraft-local`, sonst `sh -c 'echo sshcraft-local'`. Die Ausgabe enthält `sshcraft-local`, danach `Exited(Some(0))`.
  - `resize_does_not_fail`.
  - `available_shells_ends_with_custom`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-local` grün und clippy sauber.
- [ ] **Step 5: Commit** `sshcraft-local: lokales Terminal über PTY`

Keine Gegenprobe: kein Recht und kein Datenweg, nur Prozessstart.

---

### Task 10: Serielle Verbindung

**Files:**
- Create: `crates/sshcraft-local/src/serial.rs`
- Test: Unit-Tests in `serial.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct SerialSettings { pub port: String, pub baud: u32, pub data_bits: u8, pub stop_bits: u8, pub parity: Parity, pub flow: FlowControl }`.
  - `enum Parity { None, Odd, Even }`, `enum FlowControl { None, Software, Hardware }`, beide `snake_case`.
  - `impl Default`: Port leer, 115200, 8, 1, `None`, `None`.
  - `SerialSettings::validate(&self) -> Result<(), SerialError>`:
    - `baud` zwischen 50 und 4 000 000
    - `data_bits` 5 bis 8
    - `stop_bits` 1 oder 2
    - Port nicht leer
  - `pub fn list_ports() -> Vec<SerialPortInfo { name: String, description: String }>`. Fehler ergeben eine leere Liste.
  - `pub fn open(settings: &SerialSettings) -> Result<(SerialWriter, mpsc::Receiver<ShellEvent>), SerialError>`.
  - `SerialWriter::write`. `resize` ist ein No-op, die Schnittstelle kennt keine Fenstergröße.
  - `pub(crate) fn pump<R: Read + Send + 'static>(reader: R, tx: mpsc::Sender<ShellEvent>)`:
    - Lesezeit-Überschreitungen ignorieren.
    - Jeder andere Fehler ergibt `Ended(Disconnect::Lost(...))`.
  - `SerialError { Invalid(&'static str), Open(String) }`.

- [ ] **Step 1: Tests schreiben.**
  - `default_is_115200_8n1`.
  - `validate_rejects_bad_values`: Baud 0, 9 Datenbits, 3 Stoppbits, leerer Port.
  - `pump_forwards_then_reports_lost`: Ein Reader liefert zwei Blöcke, dann `TimedOut`, dann `BrokenPipe`. Ergebnis: beide Blöcke, danach `Lost`.
  - `settings_json_roundtrip`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-local` grün.
- [ ] **Step 5: Gegenprobe.** In `validate` die Grenze für `data_bits` entfernen. `validate_rejects_bad_values` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-local: serielle Verbindung`

---

### Task 11: Tabs für Host, lokales Terminal und Seriell

**Files:**
- Modify: `app/src-tauri/src/sessions.rs`, `src/commands/sessions.rs`, `src/lib.rs`, `app/src-tauri/Cargo.toml` (`sshcraft-local` nur für Desktop-Ziele)
- Create: `app/src-tauri/src/commands/local.rs`
- Test: `app/src-tauri/src/sessions_tests.rs`

**Interfaces:**
- Consumes: Tasks 8 bis 10.
- Produces:
  - `#[serde(tag = "kind", rename_all = "snake_case")] pub enum SessionTarget { Host { host_id: HostId }, Local { shell: ShellCommand }, Serial { settings: SerialSettings } }`. Unter Nicht-Desktop gibt es nur `Host`; die anderen ergeben `NotSupported`.
  - `#[async_trait] trait TermWriter: Send + Sync { async fn write(&self, data: &[u8]) -> Result<(), String>; async fn resize(&self, cols: u32, rows: u32) -> Result<(), String>; }`. Implementiert für `ShellWriter`, `LocalWriter` und `SerialWriter`. `Live.writer` hält `Arc<dyn TermWriter>`.
  - `SessionManager::open(target: SessionTarget, cols, rows, sink) -> SessionId`. `session_open` nimmt `target` statt `host_id`.
  - Wiederverbinden:
    - lokales Terminal nie: Ende des Prozesses beendet den Tab
    - seriell wie SSH bei `Lost` (der Adapter wurde abgezogen)
    - "zuletzt verbunden" nur für Hosts
  - Befehle:
    - `local_shells() -> Vec<ShellCommand>` (mit der eigenen Shell aus den Einstellungen)
    - `serial_ports() -> Vec<SerialPortInfo>`
    - `serial_last() -> SerialSettings`
    - `serial_remember(settings)`
  - Einstellungen auf dem Gerät, nicht synchronisiert: `terminal.local_shell` (JSON `ShellCommand`, optional) und `serial.last`.

- [ ] **Step 1: Tests schreiben.**
  - `local_target_runs_and_ends`: Die Echo-Shell aus Task 9 liefert `Connected`, Daten, dann `Ended { reason: Exited }` und kein `Reconnecting`.
  - `host_target_unchanged`: Die bisherigen Sitzungstests mit `SessionTarget::Host` sind grün.
  - `serial_with_invalid_settings_ends_with_error`.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test -p sshcraft-app` grün.
- [ ] **Step 5: Commit** `App: Tabs für lokales Terminal und serielle Verbindung`

---

### Task 12: Oberfläche für Zugangsdaten und Verbindungsoptionen

**Files:**
- Create: `app/src/components/ConnectionFields.tsx`, `IdentityForm.tsx`, `AuthPromptDialog.tsx`, `PasswordField.tsx` (mit Tests `*.test.tsx`)
- Modify: `app/src/components/HostForm.tsx`, `GroupForm.tsx`, `app/src/forms/hostForm.ts` (+ Test), `app/src/pages/KeysPage.tsx` (+ Test), `app/src/pages/SettingsPage.tsx`, `app/src/api.ts`, `app/src/App.tsx` (Dialog für `auth-prompt`), `app/src/i18n/{en,de}.json`

**Interfaces:**
- Consumes: Befehle und Ansichten aus Task 8.
- Produces:
  - `PasswordField`:
    - Ohne gesetztes Passwort ein Eingabefeld.
    - Mit gesetztem Passwort "Gespeichert" sowie Knöpfe "Ändern" und "Entfernen".
    - Liefert `PasswordChange`.
    - Das Feld zeigt nie ein gespeichertes Passwort, weil es keines bekommt.
  - Host- und Gruppenformular:
    - Feld Identität: Auswahl, "keine" erbt.
    - Feld Passwort.
    - Aufklappbereich "Verbindung" (`ConnectionFields`):
      - Sprung-Hosts: geordnete Liste mit Hinzufügen, Entfernen, Hoch und Runter; Auswahl aus allen Hosts außer dem eigenen; "Erben" oder "Keine".
      - Proxy: Erben, Direkt, HTTP oder SOCKS5, mit Adresse, Port, Benutzer und Passwort.
      - Agent-Weiterleitung: Erben, An oder Aus. Bei "An" ein Warnhinweis: Wer Root auf dem Server hat, kann den Agenten benutzen, solange die Sitzung offen ist.
      - Umgebungsvariablen: Zeilen aus Name und Wert. Ungültige Namen werden markiert und nicht gespeichert.
  - Speichern schickt erst Host oder Gruppe, dann die Passwortänderungen über `password_set` und `password_clear`.
  - Auf der Seite Schlüssel ein Abschnitt "Identitäten" mit Liste, Anlegen, Bearbeiten und Löschen über `IdentityForm` (Name, Benutzer, Schlüssel, Passwort, Ort).
  - `AuthPromptDialog`:
    - zeigt Host, Benutzer, Anweisungen und je Zeile ein Feld, verdeckt bei `echo == false`
    - "Anmelden" schickt `answer_auth(id, answers)`, "Abbrechen" schickt `answer_auth(id, null)`
    - wird über `withdrawn` geschlossen wie der Host-Schlüssel-Dialog
  - Einstellungen:
    - "Schlüssel und Passwörter synchronisieren" (Text des bisherigen Schalters).
    - Unter "Terminal" die eigene Shell: Programm und Argumente, eine Zeile je Argument.
  - `hostForm.ts`: Umwandlung der neuen Felder in beide Richtungen.

- [ ] **Step 1: Tests schreiben.**
  - `hostForm.test.ts`: Proxy `inherit`/`direct`/`http` hin und zurück; Sprung-Liste `inherit` gibt `null`, `none` gibt `[]`; Umgebungsvariablen mit ungültigem Namen fallen beim Speichern weg.
  - `PasswordField.test.tsx`: Gespeichert, dann Ändern, ergibt `Set`; Entfernen ergibt `Clear`; unberührt ergibt `Keep`.
  - `AuthPromptDialog.test.tsx`: eine verdeckte und eine sichtbare Zeile, die Antworten in Reihenfolge, Abbrechen gibt `null`.
  - `KeysPage.test.tsx`: Eine Identität wird gelistet und gelöscht.
  - Der Schutz gegen fest verdrahtete Texte bleibt grün.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `npm test` grün, `npm run build` ohne Fehler.
- [ ] **Step 5: Commit** `Oberfläche: Identitäten, Passwörter, Sprung-Hosts, Proxy, Agent-Weiterleitung, Umgebung`

---

### Task 13: Oberfläche für lokales Terminal und Seriell

**Files:**
- Modify:
  - `app/src/pages/NewTabPage.tsx`
  - `app/src/state/tabs.ts` (+ Test)
  - `app/src/components/TerminalView.tsx`
  - `app/src/palette/CommandPalette.tsx`
  - `app/src/api.ts`
  - `app/src/i18n/{en,de}.json`
- Create: `app/src/components/SerialForm.tsx` (+ Test)

**Interfaces:**
- Consumes: Befehle aus Task 11.
- Produces:
  - `SessionTab` trägt `target: SessionTarget` statt `hostId`. Der Titel ist der Hostname, das Label der Shell oder `"<Port> (<Baud>)"`.
  - Aktionen: `open` und `pick` nehmen `target`.
  - Die Neuer-Tab-Seite bekommt unter der Hostsuche zwei Abschnitte:
    - "Lokales Terminal": ein Knopf je Shell aus `local_shells`.
    - "Seriell": `SerialForm` mit Portauswahl aus `serial_ports` und Knopf "Aktualisieren", dazu Baud (Liste der üblichen Werte plus freie Eingabe), Datenbits, Stoppbits, Parität und Flusskontrolle. Vorbelegt aus `serial_last`; beim Öffnen wird `serial_remember` aufgerufen.
  - Auf dem Handy, wo beide Listen leer sind, fehlen beide Abschnitte.
  - Die Befehlspalette zeigt "Lokales Terminal: <Label>" je Shell, nach den Hosts.

- [ ] **Step 1: Tests schreiben.**
  - `tabs.test.ts`: `open` mit `Local`-Ziel, Titel aus dem Label.
  - `SerialForm.test.tsx`: vorbelegt aus `serial_last`, Öffnen liefert die Einstellungen, ungültiger Baudwert sperrt den Knopf.
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `npm test` grün und `npm run build` ohne Fehler.
- [ ] **Step 5: Commit** `Oberfläche: lokales Terminal und serielle Verbindung im neuen Tab`

---

### Task 14: Import, Export und Doku

**Files:**
- Modify: `crates/sshcraft-core/src/sshconfig.rs` (+ `sshconfig_tests.rs`), `src/import/mod.rs`, `src/export.rs` (+ `export_tests.rs`)
- Modify: `docs/hosts.md`, `docs/sync.md`, `docs/protocol.md`, `docs/security.md`, `README.md`
- Modify: `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md` (Zeile 9 auf "fertig, auf `main`")
- Create: `docs/connections.md`

**Interfaces:**
- Produces:
  - Import aus `~/.ssh/config`:
    - `ProxyJump a,b` setzt `jump_hosts`, wenn jeder Name ein Alias aus demselben Import ist oder ein vorhandener Host mit diesem Namen am selben Ort. Sonst bleibt die Warnung wie bisher.
    - `ProxyCommand` bleibt eine Warnung.
    - `ForwardAgent yes` setzt `agent_forwarding: Some(true)`.
    - `SetEnv A=1 B="x y"` setzt `env` (Anführungszeichen wie OpenSSH).
    - `ImportCandidate` bekommt dazu `jump_aliases: Vec<String>`; `apply_import` übersetzt sie nach dem Anlegen in `HostId`s.
  - Export:
    - JSON enthält die neuen Einstellungen.
    - Statt Passwort-Ids steht nur `has_password: true`, Identitäten erscheinen nur mit Namen.
    - CSV bekommt keine neuen Spalten.
  - `docs/connections.md`:
    - Reihenfolge der Anmeldung; wann ein gespeichertes Passwort antwortet.
    - Identitäten und ihre Vererbung.
    - Sprung-Hosts: eigene Zugangsdaten je Hop, die eigenen Sprung- und Proxy-Einstellungen eines Sprung-Hosts gelten nicht, höchstens 8.
    - Proxy: der erste Hop; HTTP-Zugangsdaten gehen unverschlüsselt an den Proxy.
    - Agent-Weiterleitung: Risiko, nur das Ziel, nur der System-Agent.
    - Umgebungsvariablen: Der Server nimmt nur, was `AcceptEnv` erlaubt.
    - Seriell und lokales Terminal.
    - Post-Quanten-Schlüsseltausch.
  - README-Tabelle "Dokumentation": neue Zeile.
  - `docs/sync.md`: Passwörter und Identitäten, der Schalter heißt "Schlüssel und Passwörter".
  - `docs/protocol.md`: die Item-Arten `identity` und `password`.
  - `docs/security.md`: Passwörter versiegelt, nie zurück an die Oberfläche; Agent-Weiterleitung.

- [ ] **Step 1: Tests schreiben.**
  - `proxyjump_to_imported_alias_sets_jump_hosts`.
  - `proxyjump_to_unknown_keeps_warning`.
  - `forwardagent_and_setenv_imported`.
  - `export_json_has_no_password_ids` (Gegenprobe Pflicht, Datenweg).
- [ ] **Step 2–4:** Erst rot, dann umsetzen, dann `cargo test --workspace` und `npm test` grün.
- [ ] **Step 5: Gegenprobe.** Im Export die Passwort-Id stehen lassen. `export_json_has_no_password_ids` muss rot werden.
- [ ] **Step 6: Commit** `Import, Export und Doku für Verbindungen`

---

## Abschluss

1. Volle Prüfung:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - in `app`: `npm ci`, `npm test`, `npm run build`
   - Installer: `npm run tauri build`
2. Finales Review des ganzen Zweigs auf dem stärksten Modell, mit der Liste unter "Review Focus". Alle Befunde beheben, auch die kleinen.
3. Lokal in `main` mergen, Installer in den Hauptordner kopieren, Arbeitsbaum und Zweig entfernen. Im Fahrplan Zeile 9 auf "fertig, auf `main`" setzen.
