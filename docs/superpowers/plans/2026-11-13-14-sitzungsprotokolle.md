# SSHCraft Teilplan 14: Sitzungsprotokolle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sitzungen über SSH, seriell und im lokalen Terminal werden auf Wunsch aufgezeichnet, versiegelt gespeichert, lassen sich mit Lesezeichen und Kommentaren versehen, durchsuchen, ansehen, exportieren und löschen, werden nach einer einstellbaren Frist entfernt und synchronisieren über den persönlichen Bereich eines Sync-Servers.

**Architecture:**
- **Aufzeichnung in Rust:** Ein `Recorder` hängt sich als Hülle um die `SessionSink` einer Sitzung und schreibt die Ausgabe (nur was der Server oder die Shell sendet, keine Eingabe) in Teilen von höchstens 32 KiB in die lokale Datenbank, jeder Teil mit dem Tresor versiegelt. Ein Protokoll umfasst die ganze Lebenszeit eines Tabs samt Wiederverbindungen.
- **Speicher (Schema 10):** `session_logs` hält je Protokoll die Kopfdaten (versiegelt: Titel, Lesezeichen, Kommentare), `session_log_chunks` die versiegelten Teile. Beide haben eine Spalte `location` wie Hosts: Ein Protokoll liegt, wo sein Host liegt, und zieht mit ihm um.
- **Sync:** zwei neue Arten im bestehenden Datensatz-Sync, `log` (Kopf) und `log_chunk` (Teil, unveränderlich). Sie gehen immer in den persönlichen Bereich, nie in Freigaben, und nur, wenn am Server `sync_logs` an ist (eigener Schalter, Vorgabe aus). Ein Teil passt mit 32 KiB plus Base64 unter die 64 KiB, die der Server je Datensatz annimmt; der Server bleibt unverändert.
- **Oberfläche:** Die Seite *Protokolle* listet, filtert, durchsucht und löscht; die Ansicht spielt ein Protokoll in ein schreibgeschütztes xterm ein, mit Lesezeichen und Kommentaren am Rand. Im Terminal zeigt ein Punkt die laufende Aufzeichnung, ein Tastenkürzel setzt ein Lesezeichen.

**Tech Stack:** wie Teilplan 13; neu nur `@xterm/addon-search` (neueste Fassung per `npm view`).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`:
- Abschnitt 2 "Sitzungsprotokolle": "Aufzeichnung je Host für SSH, seriell und lokales Terminal, Lesezeichen und Kommentare, durchsuchbar, verschlüsselt und mit Sync; Ansicht auf dem Desktop." "Protokolle lassen sich löschen und exportieren, die Aufbewahrung ist einstellbar."
- Abschnitt 7: Ende-zu-Ende-Verschlüsselung, der Server sieht nur Chiffrat, Versionen und Autor.
- Abschnitt 11: nichts Geheimes im Klartext auf der Platte, nichts Geheimes in Logs.

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 13 gelten weiter (Arbeitsbaum `G:\Claude\worktrees\sshcraft-t14`, Zweig `teilplan-14-protokolle`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung, Gegenproben für Regeln, Rechte und Datenwege, lokal mergen, Texte nur über i18n, Fehler als `ErrorCode`).
- **Was aufgezeichnet wird:** nur die Ausgabe, nie die Tastatureingabe. Ein Passwort, das ohne Echo getippt wird, landet so nicht im Protokoll.
- **Ob aufgezeichnet wird:**
  - SSH-Host: `ConnectionSettings::session_log: Option<bool>`, vererbt über die Gruppen wie die anderen Felder; `None` bis oben heißt: Einstellung `session_logs`.
  - Lokales Terminal und seriell: Einstellung `session_logs`.
  - Einstellung `session_logs` (Vorgabe **aus**), `log_retention_days` (Vorgabe `0` = unbegrenzt, erlaubt `0`, `7`, `30`, `90`, `365`).
  - Ohne entsperrten Tresor wird nicht aufgezeichnet.
- **Protokoll:**
  - `LogId(Uuid)`; Kopf `LogMeta { id, title: String (1..=200), kind: LogKind (ssh|serial|local), host_id: Option<HostId>, started_at: i64, ended_at: Option<i64>, size: u64, truncated: bool, marks: Vec<LogMark> }`, Zeiten in Unix-Millisekunden.
  - `LogMark { id: Uuid, offset: u64, kind: MarkKind (bookmark|comment), text: String, created_at: i64 }`. Lesezeichen-Text 0 bis 100 Zeichen, Kommentar 1 bis 2000 Zeichen, keine Steuerzeichen außer `\n` im Kommentar, höchstens 500 Marken je Protokoll, `offset <= size`.
  - Teile: `seq` ab 0 lückenlos, je höchstens `CHUNK_SIZE = 32 * 1024` Bytes; ein Protokoll höchstens `MAX_LOG_SIZE = 64 MiB`, danach hört die Aufzeichnung auf und `truncated` wird gesetzt.
  - Versiegelt mit `Vault::seal_secret`, Label `log:<log id>` für den Kopf und `log-chunk:<log id>:<seq>` für einen Teil, damit ein Teil unter fremder Nummer oder fremdem Protokoll nicht aufgeht.
  - Klartext in der Datenbank nur: `id`, `host_id`, `started_at`, `ended_at`, `size`, `location`, je Teil `id`, `log_id`, `seq`, `location`.
- **Schreiben:** gepuffert, ein Teil wird geschrieben, wenn 32 KiB voll sind, spätestens 5 Sekunden nach dem ersten ungeschriebenen Byte, beim Lesezeichen und beim Ende. Ein Fehler beim Schreiben beendet nie die Sitzung, er beendet nur die Aufzeichnung.
- **Ende:** Status `Ended`, Schließen des Tabs oder Abbruch der Aufgabe setzen `ended_at`. Ein Protokoll ohne `ended_at` beim Start der App (Absturz) wird mit der Zeit seines letzten Teils beendet.
- **Aufbewahrung:** beim Start und danach stündlich werden beendete Protokolle mit `ended_at < jetzt - Tage` gelöscht, wenn `log_retention_days > 0`. Das Löschen synchronisiert wie jedes andere (es trifft also auch die anderen Geräte).
- **Löschen** einzeln, ausgewählt, alle eines Hosts, alle. Ein gelöschter Host lässt seine Protokolle stehen (Titel bleibt im Kopf).
- **Suche:** über alle Protokolle, ohne Groß- und Kleinschreibung, im bereinigten Text (`plain_text`): höchstens 200 Treffer, je Treffer `LogHit { log_id, line: u32, snippet: String (höchstens 200 Zeichen) }`. Anfrage 2 bis 200 Zeichen.
- **`plain_text(bytes) -> String`:** entfernt CSI-, OSC- (bis BEL oder ST), DCS- und andere ESC-Folgen, macht aus `\r\n` ein `\n`, ein einzelnes `\r` setzt die Zeile auf ihren Anfang zurück (Fortschrittsbalken), `\x08` löscht das Zeichen davor, andere Steuerzeichen außer `\n` und `\t` fallen weg, ungültiges UTF-8 wird ersetzt.
- **Export:** `text` (`plain_text`, `.txt`) oder `raw` (Bytes wie aufgezeichnet, `.log`, mit `cat` abspielbar), in eine Datei, die der Nutzer im Speichern-Dialog wählt.
- **Sync:**
  - `ItemKind::Log` (Tabelle `session_logs`), `ItemKind::LogChunk` (Tabelle `session_log_chunks`).
  - `ItemBody::Log { meta: LogMeta }`, `ItemBody::LogChunk { id: Uuid, log_id: LogId, seq: u32, data: Vec<u8> (b64) }`.
  - Bereich immer der persönliche; Freigaben nehmen keine Protokolle auf.
  - Neue Spalte `sync_servers.sync_logs` (Vorgabe 0). Aus: `log`- und `log_chunk`-Einträge im Ausgang werden verworfen, ankommende übersprungen und nicht gezählt. Einschalten setzt die Cursor wie beim Schlüssel-Sync auf 0 und stellt alle beendeten Protokolle dieses Servers in den Ausgang.
  - Ein Protokoll geht erst nach seinem Ende in den Ausgang: erst alle Teile, dann der Kopf. Beim Abholen wird ein Teil auch ohne Kopf gespeichert; die Liste zeigt nur Protokolle mit Kopf.
  - `location` beim Beenden = `location` des Hosts; Umzug des Hosts (`set_location`) nimmt seine Protokolle mit; `remove_server` macht sie lokal. Protokolle ohne Host bleiben auf dem Gerät.
- **Tastenkürzel** `bookmark` (Vorgabe `Ctrl+Shift+B`) setzt im fokussierten Bereich ein Lesezeichen mit der Uhrzeit als Text; damit 13 Aktionen.

## Review Focus

1. Ein Absturz oder `close` mitten in der Aufzeichnung: Teile im Puffer gehen höchstens bis zu 5 Sekunden verloren, das Protokoll erscheint trotzdem beendet und vollständig lesbar.
2. Eine sehr schnelle Ausgabe (`cat` großer Dateien): die Aufzeichnung bremst die Sitzung nicht spürbar und stoppt sauber bei 64 MiB.
3. Ein Gerät mit `sync_logs` aus und eins mit an am selben Konto: keins verliert Protokolle, keins zeigt Warnungen für übersprungene Protokoll-Datensätze.
4. Ein Protokoll eines Hosts in einer Freigabe: es landet im persönlichen Bereich, nie bei den Empfängern.
5. Ein Teil, der unter einer anderen `seq` oder einem anderen Protokoll eingespielt wird (Datenbank oder Server manipuliert): er geht nicht auf und wird übergangen.

---

### Task 1: Kern, bereinigter Text und Protokollspeicher

**Files:**
- Create: `crates/sshcraft-core/src/logtext.rs`, `crates/sshcraft-core/src/session_log.rs`, `crates/sshcraft-core/src/session_log_tests.rs`
- Modify: `crates/sshcraft-core/src/{lib.rs, model.rs (LogId, ConnectionSettings::session_log, ResolvedHost::session_log), store.rs (SCHEMA_V10, SCHEMA_VERSION = 10)}`

**Interfaces:**
- Produces: `logtext::plain_text(&[u8]) -> String`; in `session_log`: `LogMeta`, `LogMark`, `LogKind`, `MarkKind`, `LogSummary { meta: LogMeta, location: Option<ServerId> }`, `LogHit`, `LogError`;
  `create(vault, store, meta) -> Result<(), LogError>`, `append_chunk(vault, store, id, seq: u32, bytes: &[u8])`, `finish(vault, store, id, ended_at, size, truncated)`, `list(vault, store) -> Vec<LogSummary>` (neueste zuerst, Unlesbares fehlt), `meta(vault, store, id) -> Option<LogMeta>`, `read(vault, store, id) -> Vec<u8>` (Teile in Reihenfolge, Lücke oder unlesbarer Teil = Fehler `LogError::Damaged`), `save_marks(vault, store, id, Vec<LogMark>)`, `delete(store, id)`, `chunk_ids(store, id) -> Vec<String>`, `expired(store, before_ms) -> Vec<LogId>`, `finish_abandoned(vault, store) -> Vec<LogId>`, `search(vault, store, query) -> Vec<LogHit>`, `validate_marks(&[LogMark], size) -> Result<(), LogError>`.
- `ResolvedHost::session_log: Option<bool>` aus der Gruppenkette.

- [ ] Tests zuerst (rot), dann umsetzen:
  - `plain_text`: Farben (`\x1b[31mrot\x1b[0m` → `rot`), OSC-Titel mit BEL und mit ST, `a\r\nb` → `a\nb`, `50%\r100%` → `100%`, `ab\x08c` → `ac`, ungültiges UTF-8 → Ersatzzeichen, `\x07` fällt weg.
  - Speicher: Anlegen, drei Teile, `read` liefert die Bytes in Reihenfolge; Teil mit Lücke → `Damaged`; Teil, dessen versiegelter Block in die Zeile eines anderen `seq` kopiert wurde → `Damaged` (Gegenprobe: Label ohne `seq`); `list` ohne Kopf zeigt nichts; Marken außerhalb von `size`, zu lang, 501 Stück → Fehler; `delete` entfernt Kopf und Teile; `expired` nur beendete und alte; `finish_abandoned` setzt `ended_at` auf `started_at` bzw. die Zeit des Beendens, wenn kein Teil da ist.
  - Suche: Treffer über Teilgrenzen hinweg (Wort halb im ersten, halb im zweiten Teil), ohne Großschreibung, Grenze 200, Anfrage unter 2 Zeichen → leer.
  - Auflösung: Host `None`, Gruppe `Some(true)` → `Some(true)`; Host `Some(false)` gewinnt.
  - Migration 9 → 10 auf einer Datenbank mit Daten.
- [ ] `cargo test -p sshcraft-core` grün, Commit "Kern: Sitzungsprotokolle speichern, durchsuchen, bereinigter Text".

### Task 2: Aufzeichnung in den Sitzungen

**Files:**
- Create: `app/src-tauri/src/recording.rs`, `app/src-tauri/src/recording_tests.rs`
- Modify: `app/src-tauri/src/{sessions.rs (open entscheidet über Aufzeichnung, SessionManager::bookmark, recording(id) -> bool), settings.rs (session_logs, log_retention_days, Aktion bookmark), lib.rs (Aufräumen beim Start und stündlich), errors.rs}`

**Interfaces:**
- Consumes: Task 1.
- Produces: `Recorder::start(store, vault, meta) -> Option<Arc<Recorder>>`, `RecordingSink { inner, recorder }: SessionSink`, `Recorder::bookmark(&self, text) -> Result<LogMark, LogError>`, `SessionManager::bookmark(id, text) -> Result<LogMark, String>`, `SessionManager::is_recording(id) -> bool`, `logs_housekeeping(store, vault, settings, now_ms)` (beendet Verwaiste, löscht Abgelaufene, stellt in den Ausgang).
- Beim Beenden: `location` des Protokolls und seiner Teile = `location` des Hosts, dann `record_local_change` für jeden Teil und den Kopf.

- [ ] Tests zuerst: Ausgabe von 70 KiB ergibt drei Teile und gleiche Bytes; Ende per Status und per `close` (Drop) beendet das Protokoll; 5-Sekunden-Grenze mit `tokio::time::pause`; 64 MiB-Grenze mit kleinerer Testgrenze (`Recorder` nimmt Grenzen als Parameter); Fehler beim Speichern (Tresor fehlt) beendet die Sitzung nicht; Host mit `session_log: Some(false)` bei Einstellung an → keine Aufzeichnung (Gegenprobe); Lesezeichen-Offset = bisher aufgezeichnete Bytes; Housekeeping löscht nur Abgelaufene und stellt die Löschung eines Protokolls an einem Server in den Ausgang.
- [ ] `cargo test -p sshcraft-app`, Commit "Aufzeichnung der Sitzungen, Lesezeichen, Aufbewahrung".

### Task 3: Befehle

**Files:**
- Create: `app/src-tauri/src/commands/logs.rs`
- Modify: `app/src-tauri/src/commands/{mod.rs, sessions.rs (session_bookmark, Rückgabe von open_session mit recording)}`, `app/src-tauri/src/lib.rs`

**Interfaces:**
- Produces Tauri-Befehle: `logs_list() -> Vec<LogSummary>`, `log_read(id) -> tauri::ipc::Response` (rohe Bytes), `log_save_marks(id, marks) -> LogMeta`, `logs_delete(ids: Vec<LogId>)`, `logs_search(query) -> Vec<LogHit>`, `log_export(id, path, format: "text"|"raw")`, `session_bookmark(id, text) -> LogMark`.
- Löschen stellt vor dem Entfernen jeden Teil und den Kopf in den Ausgang (`record_local_change`).

- [ ] Tests zuerst (`commands/logs.rs` gegen Store und Vault direkt, wie Snippets): Export `text` schreibt `plain_text`, `raw` die Bytes; Löschen eines Protokolls an einem Server stellt Kopf und Teile in den Ausgang (Gegenprobe: ohne `record_local_change` rot); Marken eines laufenden Protokolls über `log_save_marks` und Lesezeichen aus der Sitzung gehen nicht verloren (das spätere `finish` behält die Marken).
- [ ] Commit "Befehle für Sitzungsprotokolle".

### Task 4: Sync der Protokolle

**Files:**
- Modify: `crates/sshcraft-core/src/{sync_store.rs (ItemKind::Log, ItemKind::LogChunk, sync_logs, remove_server), store.rs (carry_logs in set_location), share_store.rs}`, `crates/sshcraft-sync/src/{items.rs, engine.rs}`, `app/src-tauri/src/commands/sync.rs (set_server_sync mit sync_logs, ServerInfo::sync_logs)`
- Test: `crates/sshcraft-sync/tests/logs.rs`, `crates/sshcraft-core/src/sync_store_tests.rs`

**Interfaces:**
- Consumes: Task 1, Task 2 (Beenden stellt in den Ausgang).
- Produces: `Store::update_server_flags(id, sync_enabled, sync_keys, sync_logs)`, `ServerRow::sync_logs`, `engine::is_log(kind) -> bool`.

- [ ] Tests zuerst (Integration mit `TestServer`): Gerät A zeichnet auf (Protokoll an Server, `sync_logs` an), B mit `sync_logs` an liest dasselbe Protokoll samt Marken; B mit `sync_logs` aus bekommt nichts und keine Warnung, nach dem Einschalten alles; Löschen auf A entfernt es auf B; Protokoll eines Hosts in einer Freigabe landet im persönlichen Bereich (Gegenprobe: `area_for` statt persönlich → Empfänger sieht es); Umzug des Hosts nimmt Protokolle mit; `remove_server` macht sie lokal; Teil unter fremder `seq` eingespielt → übergangen, Protokoll `Damaged`, Rest des Syncs läuft.
- [ ] `cargo test -p sshcraft-sync -p sshcraft-core`, Commit "Sync: Sitzungsprotokolle im persönlichen Bereich".

### Task 5: Seite Protokolle und Ansicht

**Files:**
- Create: `app/src/pages/LogsPage.tsx`, `app/src/pages/LogsPage.test.tsx`, `app/src/components/LogViewer.tsx`, `app/src/components/LogViewer.test.tsx`, `app/src/logs/marks.ts`, `app/src/logs/marks.test.ts`
- Modify: `app/src/{api.ts, shell/AppShell.tsx, styles.css, i18n/en.json, i18n/de.json}`, `app/package.json`

**Interfaces:**
- Consumes: Task 3.
- Produces: `marks.ts`: `pieces(size, offsets: number[], step = 4096) -> number[]` (Schnittstellen zum Einspielen), `lineForOffset(table, offset)`, `offsetForLine(table, line)`.

- [ ] Tests zuerst: Liste mit Titel, Host, Beginn, Dauer, Größe und Ort; Filter nach Host; Suche ab 2 Zeichen ruft `logs_search` und öffnet beim Klick die Ansicht mit dem Suchbegriff; Mehrfachauswahl und "Alle löschen" fragen nach; Ansicht spielt in Stücken ein und springt zu einer Marke; Kommentar anlegen und löschen speichert über `log_save_marks`; Export fragt Format und Datei.
- [ ] `npm test`, `npm run build`, Commit "Seite Protokolle mit Ansicht, Suche, Export und Löschen".

### Task 6: Einstellungen, Host-Formular, Sync-Schalter, Terminal, Doku

**Files:**
- Modify: `app/src/pages/SettingsPage.tsx` (Abschnitt Sitzungsprotokolle), `app/src/pages/sync/*` (Schalter *Protokolle synchronisieren*), Host- und Gruppenformular (Feld *Sitzung aufzeichnen*: erben, an, aus), `app/src/components/TerminalView.tsx` (Punkt bei Aufzeichnung, Kürzel `bookmark`), `app/src/shell/shortcuts.ts`, i18n, `docs/session-logs.md` (neu), `docs/sync.md`, `docs/security.md`, `docs/shortcuts.md`, `docs/hosts.md`, `README.md`, Fahrplan Zeile 14.

- [ ] Tests zuerst: Einstellung speichern; Formularfeld schreibt `session_log`; Schalter ruft `set_server_sync` mit `syncLogs`; Kürzel ruft `session_bookmark` nur im fokussierten Bereich; Punkt erscheint nur bei `recording: true`.
- [ ] Alle Tests, clippy, fmt, Commit "Einstellungen, Aufzeichnung im Terminal, Doku".
