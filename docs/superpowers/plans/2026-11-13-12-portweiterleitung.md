# SSHCraft Teilplan 12: Portweiterleitung Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lokale, entfernte und dynamische (SOCKS5) Portweiterleitungen als Regeln je Host, auf einer eigenen Seite mit Start und Stopp per Doppelklick und einem Assistenten zum Anlegen.

**Architecture:** Eine Regel gehört zu einem Host und steht im Host selbst (`Host.forwards`), damit sie ohne eigenen Sync-Datentyp mit dem Host synchronisiert, geteilt und gelöscht wird. Der SSH-Kern bekommt die Weiterleitungen selbst (`tunnel.rs`: Listener, `direct-tcpip`, `tcpip-forward`, SOCKS5), die App einen `ForwardManager`, der je laufender Regel eine eigene Verbindung über denselben Weg wie ein Terminal aufbaut (Sprung-Hosts, Proxy, Schlüssel, Passphrase, Windows Hello), Status als Ereignis meldet und nach Verbindungsverlust wie ein Terminal neu verbindet. Die Oberfläche ersetzt die Platzhalterseite "Portweiterleitung".

**Tech Stack:** wie Teilplan 11; russh 0.65.1 (`Handle::channel_open_direct_tcpip`, `tcpip_forward`, `cancel_tcpip_forward`, Handler `server_channel_open_forwarded_tcpip`; Testserver `channel_open_direct_tcpip`, `tcpip_forward`, `Handle::channel_open_forwarded_tcpip`). Keine neuen Abhängigkeiten.

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`, Abschnitt 2 "Portweiterleitung": lokal, entfernt und dynamisch (SOCKS), Regeln auf einer eigenen Seite, Start und Stopp per Doppelklick, Assistent zum Anlegen. Dazu Abschnitt 11: nichts Geheimes im Klartext.

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 11 gelten weiter: Arbeitsbaum `G:\Claude\worktrees\sshcraft-t12`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8, neueste Paketversionen, Gegenproben für Regeln, Rechte und Datenwege, lokal mergen, Texte nur über `i18n/{en,de}.json`, Fehler aus Rust nur als `ErrorCode`.
- **Regel:** `ForwardRule { id: ForwardId (Uuid), name: String, kind: ForwardKind, bind_address: String, bind_port: u16, target_host: String, target_port: u16 }`, `ForwardKind { Local, Remote, Dynamic }` (serde snake_case).
  - Ports 1 bis 65535; bei `Dynamic` sind `target_host` leer und `target_port` 0.
  - Adressen 1 bis 253 Zeichen ohne Leer- und Steuerzeichen; Name höchstens 100 Zeichen.
  - Höchstens 50 Regeln je Host, IDs eindeutig. Ungültig → `ForwardError`, beim Speichern `ErrorCode::InvalidForward`.
  - Vorgabe für `bind_address`: `127.0.0.1` (lokal, dynamisch), `localhost` (entfernt).
- **Speicher:** Spalte `hosts.forwards TEXT NOT NULL DEFAULT '[]'`, Schema 8. Ein nicht lesbarer Wert ergibt `[]`. Der Host-Sync trägt die Regeln mit (`#[serde(default)]`), Export (CSV/JSON) lässt sie weg.
- **Laufzeit:**
  - Eine laufende Regel hat eine eigene SSH-Verbindung.
  - Höchstens 256 gleichzeitige Verbindungen je Regel (`MAX_TUNNEL_CONNECTIONS`), weitere werden sofort geschlossen.
  - SOCKS5 nur ohne Anmeldung (Methode 0x00) und nur `CONNECT`, Ziel als IPv4, IPv6 oder Name. Anderes wird mit dem passenden SOCKS-Fehler beantwortet und geschlossen. Kein SOCKS4.
  - Port belegt → `ErrorCode::PortInUse`, kein neuer Versuch. Server lehnt `tcpip-forward` ab → `ErrorCode::ForwardRefused`, kein neuer Versuch. Verbindung verloren → neu verbinden nach `policy::next_reconnect_delay` wie ein Terminal.
  - Löschen eines Hosts stoppt seine Regeln. Eine laufende Regel kann nicht bearbeitet werden (die Oberfläche bietet erst "Stoppen"). Kommt eine Änderung per Sync, läuft die alte Regel bis zum Stopp weiter.
- **Status** (Ereignis `forward-status`, Befehl `forward_statuses`): `{ rule_id, state: "starting" | "running" | "reconnecting" | "stopped" | "failed", attempt?, delay_secs?, code?, detail?, connections }`.
- **Sicherheit:** Eine Bindung an eine Adresse außer Loopback (`127.0.0.1`, `::1`, `localhost`) zeigt im Assistenten einen Warnhinweis (`forwarding.exposedWarning`), ist aber erlaubt.

## Review Focus

1. **Verbindung bricht ab, während Daten fließen:** Erwartet wird, dass der Listener schließt, der Status `reconnecting` zeigt und nach dem Wiederverbinden wieder lauscht, ohne dass der Port hängen bleibt. Test in Task 3.
2. **Kaputte oder feindliche SOCKS-Clients:** abgeschnittener Handshake, unbekannte Version, `BIND`/`UDP`, Name mit 0 Bytes, IPv6. Erwartet wird eine SOCKS-Fehlerantwort und Schließen, kein Hängen und kein Absturz. Tests in Task 2.
3. **Stopp während des Verbindens** (Passphrase- oder Host-Key-Frage offen): Erwartet wird, dass die Frage verschwindet und nichts weiterlauscht. Test in Task 3.
4. **Port schon belegt oder Server verbietet Weiterleitung:** Erwartet wird ein klarer Fehler und kein Neuversuch in Schleife. Tests in Task 2 und 3.
5. **Ältere Daten und Sync:** Host ohne `forwards`, Regel mit Port 0 oder 70000 per Sync. Erwartet wird Laden ohne Absturz, ungültige Regeln werden beim Laden verworfen. Test in Task 1.

---

## Dateistruktur

```
crates/sshcraft-core/src/forward.rs        ForwardRule, ForwardKind, ForwardId, validate (neu)
crates/sshcraft-core/src/model.rs          Host.forwards
crates/sshcraft-core/src/store.rs          Schema 8, Spalte forwards
crates/sshcraft-ssh/src/socks.rs           SOCKS5-Handshake (neu)
crates/sshcraft-ssh/src/tunnel.rs          lokale, entfernte, dynamische Weiterleitung (neu)
crates/sshcraft-ssh/src/lib.rs             Session::forward_*, Session::closed, Handler forwarded-tcpip
crates/sshcraft-ssh/src/testing.rs         Testserver: direct-tcpip, tcpip-forward
app/src-tauri/src/forwards.rs              ForwardManager (neu)
app/src-tauri/src/commands/forwards.rs     forward_start, forward_stop, forward_statuses (neu)
app/src/pages/ForwardingPage.tsx           Regelseite (neu)
app/src/components/ForwardWizard.tsx       Assistent (neu)
app/src/forms/forwardForm.ts               Prüfung, ssh-Entsprechung (neu)
docs/forwarding.md (neu), docs/security.md, README.md, Fahrplan
```

---

### Task 1: Kern: Regeln am Host

**Files:**
- Create: `crates/sshcraft-core/src/forward.rs` (mit `#[cfg(test)] mod tests`)
- Modify: `crates/sshcraft-core/src/{model.rs, store.rs, lib.rs}`, alle Stellen, die `Host { … }` bauen (`forwards: Vec::new()`), `app/src/api.ts` (`Host.forwards`, `ForwardRule`)
- Test: `forward.rs`, `store_tests.rs`, `crates/sshcraft-sync/tests/items.rs`

**Interfaces:**
- Produces: `forward::{ForwardRule, ForwardKind, ForwardId, ForwardError, validate(rule) -> Result<(), ForwardError>, validate_all(rules: &[ForwardRule]) -> Result<(), ForwardError>, MAX_FORWARDS_PER_HOST = 50, is_loopback(address: &str) -> bool}`; `Host.forwards: Vec<ForwardRule>` mit `#[serde(default)]`.

- [ ] **Step 1: Failing tests:** `validate` lehnt Port 0 und leere Adresse bei `Local`, Leerzeichen in der Adresse, Name mit 101 Zeichen, `Dynamic` mit Ziel ab und nimmt gültige Regeln aller drei Arten an; `validate_all` lehnt 51 Regeln und doppelte IDs ab; `is_loopback` für `127.0.0.1`, `127.1.2.3`, `::1`, `localhost`, `LOCALHOST` wahr, für `0.0.0.0`, `192.168.1.5`, `*`, `` falsch. Store: `host_forwards_round_trip`, `old_host_row_has_no_forwards` (Spalte mit `'garbage'` → `[]`), `invalid_forward_from_sync_is_dropped_on_load` (JSON mit Port 70000 und einer gültigen Regel → nur die gültige). Sync: `host_item_carries_forwards`.
- [ ] **Step 2: Run, rot.**
- [ ] **Step 3: Implement.** Laden: Regeln einzeln parsen, ungültige verwerfen. `save_host` prüft `validate_all` (Fehler `InvalidForward` in `errors.rs`).
- [ ] **Step 4: Run, grün; clippy, fmt.**
- [ ] **Step 5: Commit** `Portweiterleitung: Regeln am Host, Schema 8`

### Task 2: SSH-Kern: Tunnel und SOCKS5

**Files:**
- Create: `crates/sshcraft-ssh/src/socks.rs`, `crates/sshcraft-ssh/src/tunnel.rs`, `crates/sshcraft-ssh/tests/tunnel.rs`
- Modify: `crates/sshcraft-ssh/src/{lib.rs, testing.rs}`

**Interfaces:**
- Produces:
  - `socks::read_request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<(String, u16), SocksError>` (beantwortet Methoden-Auswahl; Fehlerantwort und `Err` bei allem anderen); `socks::reply_success(stream, bound: SocketAddr)`, `socks::reply_failure(stream, code: u8)`.
  - `tunnel::Tunnel` (läuft, bis gedroppt oder `stop()`): `bound_port() -> u16`, `connections() -> usize`.
  - `Session::forward_local(&self, bind_address: &str, bind_port: u16, target_host: &str, target_port: u16) -> Result<Tunnel, SshError>`, `Session::forward_dynamic(&self, bind_address: &str, bind_port: u16) -> Result<Tunnel, SshError>`, `Session::forward_remote(&self, bind_address: &str, bind_port: u16, target_host: &str, target_port: u16) -> Result<Tunnel, SshError>`.
  - `Session::closed(&self) -> impl Future<Output = ()>` (fertig, sobald die Verbindung zum Ziel oder zu einem Sprung-Host zu ist).
  - `SshError::PortInUse`, `SshError::Bind(String)`, `SshError::ForwardRefused`.
  - Testserver: nimmt `direct-tcpip` an (verbindet zu `host:port` auf 127.0.0.1), nimmt `tcpip-forward` an (lauscht und öffnet je Verbindung `forwarded-tcpip`), `TestAuth.refuse_forwarding: bool`.
- [ ] **Step 1: Failing tests:** SOCKS (`socks.rs` mod tests, mit `tokio::io::duplex`): IPv4-, IPv6- und Namensziel; Version 4 → Fehler; Methode ohne 0x00 → Antwort `05 FF`; Kommando `BIND` → Antwort mit 0x07; Name der Länge 0 → Fehler; abgeschnittener Handshake → Fehler ohne Hängen (Zeitlimit 5 s im Code). `tests/tunnel.rs`: `local_forward_reaches_target` (Echo-Server, Bytes hin und zurück), `dynamic_forward_speaks_socks5`, `remote_forward_reaches_local_target`, `bound_port_in_use_is_port_in_use`, `refused_remote_forward_is_forward_refused`, `closed_resolves_when_server_stops`, `dropping_tunnel_frees_port` (danach neu bindbar). Gegenprobe: `CONNECT`-Prüfung entfernt → `BIND`-Test rot.
- [ ] **Step 2: Run, rot.**
- [ ] **Step 3: Implement.** Kopieren mit `copy_bidirectional` über `channel.into_stream()`. Entfernte Weiterleitung: Ziel je `(address, port)` in einer geteilten Tabelle im `ClientHandler`; ein `forwarded-tcpip` ohne Eintrag wird abgelehnt. `stop()`/Drop: Listener schließen, `cancel_tcpip_forward`, laufende Verbindungen abbrechen.
- [ ] **Step 4: Run, grün; clippy, fmt.**
- [ ] **Step 5: Commit** `SSH-Kern: lokale, entfernte und dynamische Weiterleitung`

### Task 3: App: ForwardManager und Befehle

**Files:**
- Create: `app/src-tauri/src/forwards.rs`, `app/src-tauri/src/forwards_tests.rs`, `app/src-tauri/src/commands/forwards.rs`
- Modify: `app/src-tauri/src/{sessions.rs (Context und connect_session pub(crate)), state.rs, lib.rs, errors.rs}`, `app/src-tauri/src/commands/data.rs` (`delete_host` stoppt Regeln, `save_host` prüft), `app/src/api.ts`

**Interfaces:**
- Produces: `ForwardManager::start(host_id, rule_id, sink: Arc<dyn ForwardSink>)`, `stop(rule_id)`, `stop_host(host_id)`, `statuses() -> Vec<ForwardStatus>`; `ForwardSink::status(ForwardStatus)`; Befehle `forward_start(host_id, rule_id)`, `forward_stop(rule_id)`, `forward_statuses()`; `ErrorCode::{PortInUse, BindFailed, ForwardRefused, InvalidForward}`.
- [ ] **Step 1: Failing tests** (`forwards_tests.rs` mit dem Testserver aus Task 2 und den Fixtures aus `sessions_tests.rs`): `local_rule_runs_and_stops` (Status `starting` → `running`, Echo durch den Port, `stop` → `stopped` und Port frei); `port_in_use_fails_without_retry`; `lost_connection_reconnects_and_listens_again`; `stop_while_waiting_for_host_key_withdraws_the_question`; `deleting_host_stops_its_rules`; `unknown_rule_fails_with_not_found`; `starting_twice_keeps_one`.
- [ ] **Step 2: Run, rot.**
- [ ] **Step 3: Implement.** Ein `tokio::spawn` je Regel; Schleife: verbinden (`connect_session`), Tunnel öffnen, auf `closed()` oder Stopp warten, bei Verlust `next_reconnect_delay`. Fragen (Host-Key, Passphrase) laufen über denselben Prompter wie ein Tab, mit einer eigenen Kennung je Regel, damit Stopp sie zurückzieht.
- [ ] **Step 4: Run, grün; clippy, fmt.**
- [ ] **Step 5: Commit** `App: Weiterleitungen starten, stoppen, neu verbinden`

### Task 4: Oberfläche: Regelseite und Assistent

**Files:**
- Create: `app/src/forms/forwardForm.ts` (+ Test), `app/src/pages/ForwardingPage.tsx` (+ Test), `app/src/components/ForwardWizard.tsx` (+ Test)
- Modify: `app/src/shell/AppShell.tsx` (Seite statt Platzhalter, Ereignis `forward-status`), `app/src/pages/PlaceholderPage.tsx` (ohne `forwarding`), `app/src/i18n/{en,de}.json`, `app/src/styles.css`

**Interfaces:**
- Consumes: Task 1 (`Host.forwards`, `saveHost`), Task 3 (`forwardStart`, `forwardStop`, `forwardStatuses`, Ereignis).
- Produces:
  - `forwardForm.ts`: `type ForwardDraft`, `emptyDraft(kind)`, `validateDraft(draft) -> string | null` (Übersetzungsschlüssel des ersten Fehlers, gleiche Regeln wie Rust), `toRule(draft, id)`, `sshEquivalent(rule) -> string` (`-L 127.0.0.1:8080:db:5432`, `-R localhost:9000:127.0.0.1:3000`, `-D 127.0.0.1:1080`), `isExposed(rule) -> boolean`.
  - `ForwardingPage`: Liste aller Regeln aller Hosts, nach Host gruppiert, je Zeile Statuspunkt, Name oder Kurzform, `sshEquivalent`, Zahl der Verbindungen. Doppelklick und Enter starten oder stoppen. Kontextmenü: Starten/Stoppen, Bearbeiten (nur gestoppt), Löschen (mit Bestätigung). Knopf "Neue Regel" öffnet den Assistenten. Fehler aus dem Status stehen unter der Zeile.
  - `ForwardWizard { hosts, rule?: { hostId, rule }, onDone, onCancel }`: Schritt 1 Art (drei Karten mit je einem Satz Erklärung), Schritt 2 Host, Schritt 3 Adressen und Ports (Vorgaben aus den Global Constraints, Warnhinweis bei `isExposed`), Schritt 4 Übersicht mit `sshEquivalent` und Speichern. Beim Bearbeiten beginnt er bei Schritt 3.
- [ ] **Step 1: Failing tests:** `forwardForm.test.ts` (Validierung wie Task 1, `sshEquivalent` für alle drei Arten, `isExposed`); `ForwardWizard.test.tsx` (Durchlauf lokal speichert den Host mit der neuen Regel; `0.0.0.0` zeigt die Warnung; ungültiger Port blockiert "Weiter"); `ForwardingPage.test.tsx` (Doppelklick ruft `forward_start`, bei laufender Regel `forward_stop`; laufende Regel bietet kein "Bearbeiten"; Status `failed` mit `port_in_use` zeigt den Text).
- [ ] **Step 2: Run, rot.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** vitest, tsc, `no-hardcoded-text`.
- [ ] **Step 5: Commit** `Oberfläche: Regelseite und Assistent für Weiterleitungen`

### Task 5: Doku und Fahrplan

**Files:**
- Create: `docs/forwarding.md` (Englisch)
- Modify: `README.md` (Tabelle), `docs/security.md` (Bindung außer Loopback, SOCKS ohne Anmeldung), `docs/sync.md` (Regeln reisen mit dem Host; ältere Fassungen verlieren sie beim Bearbeiten des Hosts), Fahrplan Zeile 12

- [ ] **Step 1:** Doku schreiben.
- [ ] **Step 2:** Volle Prüfung: `cargo test --workspace`, clippy, fmt, `npm test`, `tsc`.
- [ ] **Step 3: Commit** `Doku: Portweiterleitung, Fahrplan`
