# SSHCraft Teilplan 8: Hosts und Ordnung Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gruppen lassen sich verschachteln und vererben über alle Ebenen, Hosts und Gruppen tragen Tags und ein Symbol nach Betriebssystem, Hosts kommen aus `known_hosts`, Schlüsseldateien, CSV, PuTTY, MobaXterm und SecureCRT herein und gehen als CSV und JSON ohne Geheimnisse hinaus.

**Architecture:** `Group` bekommt `parent_id` und `tags`, `Host` bekommt `tags` und `os`; die Auflösung einer Verbindung läuft über die ganze Kette Host, Gruppe, Elterngruppe bis zur obersten. Ort und Freigabebereich einer Untergruppe folgen ihrer Elterngruppe, so wie ein Host seiner Gruppe folgt. Alle Importe erzeugen dieselben `ImportCandidate`s (Name, Gruppenpfad, Einstellungen, Tags, Schlüsselhinweis, Warnungen), die ein gemeinsames `apply_import` anlegt; die Oberfläche hat einen Import-Dialog mit Quellenwahl und Vorschau. Das Symbol wird nach der ersten erfolgreichen Verbindung über einen eigenen Exec-Kanal (`/etc/os-release`, `uname`) erkannt und lässt sich von Hand setzen.

**Tech Stack:** wie Teilplan 7; neu: `csv` 1.4.0, `winreg` 0.56.0 (nur Windows), `ssh-key` mit Feature `ppk` (dieselbe Fassung, die `russh` nutzt, derzeit `0.7.0-rc.11`), `tauri-plugin-dialog` in der neuesten Fassung für Tauri 2 (Rust 2.x, npm `@tauri-apps/plugin-dialog` 2.8.4; die Rust-Reihe 3.0.0-alpha gehört zu Tauri 3), npm `simple-icons` 16.37.0 (CC0) für Distributionslogos. Vor dem Eintragen jeweils neu prüfen (`cargo info`, `npm view`).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`, Abschnitt 2 "Hosts und Ordnung" (Untergruppen mit Vererbung über alle Ebenen, Tags, Host-Symbole, Importe, Export ohne Geheimnisse), Abschnitt 4 (Gruppe, Host, Ort; Untergruppen Pflicht), Abschnitt 5 (Schlüsselquellen), Abschnitt 8 (Teilen: eine Gruppe "mit allen Hosts" nimmt ihre Untergruppen mit).

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 7 gelten weiter (Arbeitsbaum unter `G:\Claude\worktrees`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8, neueste Paketversionen, `cargo update` ohne `--workspace`, `npm outdated`, Gegenproben für Regeln, Rechte und Datenwege, ohne Remote nach grünen Tests lokal in `main` mergen).
- **Export ohne Geheimnisse:** kein privater Schlüssel, keine Passphrase, nichts aus dem Tresor; ein Schlüssel erscheint nur mit Name und Fingerprint.
- **Importe lesen nur:** keine Importquelle wird verändert. Dateien aus fremden Programmen sind ungeprüfte Eingaben: kaputte Zeilen werden übersprungen und gemeldet, nie bricht der ganze Import ab. Keine Pfade, Passwörter oder Schlüssel aus Importdateien landen im Log.
- **Passwörter aus Importquellen** (MobaXterm, SecureCRT speichern sie teils) werden nicht übernommen; Passwort-Anmeldung kommt erst mit Teilplan 9. Der Import sagt das in einer Warnung.
- **Keine Zyklen:** eine Gruppe kann nicht in sich selbst oder in eine eigene Untergruppe gehängt werden, weder lokal noch durch einen Datensatz vom Server.
- Oberflächentexte nur über die Übersetzung (en, de), Fehler aus Rust nur als `ErrorCode`.

## Review Focus

1. **Ein Datensatz vom Server hängt eine Gruppe in ihre eigene Untergruppe** (anderes Gerät, gleichzeitige Änderungen, böswilliger Mitschreiber): kein Zyklus im Store, keine Endlosschleife bei der Auflösung, der Datensatz wird abgelehnt und gemeldet. Test in Task 2.
2. **Eine Gruppe "mit allen Hosts" geteilt, darunter Untergruppen:** Untergruppen und deren Hosts gehen mit, eine später angelegte Untergruppe auch; ein Empfänger mit Leserecht kann sie nicht anlegen oder verschieben. Tests in Task 3.
3. **Fremde Importdateien mit Unsinn** (falsches Format, abgeschnittene Zeilen, riesige Werte, Port 0 oder 99999, gehashte `known_hosts`-Namen): Import läuft weiter, meldet die Zeile, legt nichts Halbes an. Tests in Task 5 bis 8.
4. **Export enthält nie Geheimnisse**, auch nicht für Hosts mit gespeichertem Schlüssel oder aus Freigaben. Test in Task 9.
5. **Gleicher Host zweimal importiert** (aus zwei Quellen oder zweimal dieselbe Datei): kein Duplikat, der vorhandene bleibt. Test in Task 4.

---

## Dateistruktur

```
crates/sshcraft-core/src/model.rs        Group.parent_id/tags, Host.tags/os, HostOs, resolve über Ketten
crates/sshcraft-core/src/store.rs        Schema 5, Gruppenbaum, Zyklusprüfung
crates/sshcraft-core/src/import/mod.rs   ImportCandidate, apply_import, Duplikate, Gruppenpfade
crates/sshcraft-core/src/import/{knownhosts,keyfiles,csv,putty,mobaxterm,securecrt}.rs
crates/sshcraft-core/src/export.rs       CSV und JSON ohne Geheimnisse
crates/sshcraft-core/src/sshconfig.rs    auf import::ImportCandidate umgestellt
crates/sshcraft-ssh/src/lib.rs           Session::exec für die Erkennung des Betriebssystems
crates/sshcraft-sync/src/{engine,shares}.rs  Untergruppen folgen Ort und Bereich, Teilen mit Untergruppen
app/src-tauri/src/commands/{import,export}.rs, sessions.rs (Erkennung)
app/src/components/ImportDialog.tsx, ExportDialog.tsx, HostIcon.tsx, TagInput.tsx
app/src/pages/HostsPage.tsx              Baum mit Untergruppen, Tags, Filter, Symbole
docs/hosts.md                            Ordnung, Importe, Export
```

---

### Task 1: Datenmodell und Store

**Files:**
- Modify: `crates/sshcraft-core/src/model.rs`, `src/store.rs`
- Test: `crates/sshcraft-core/src/store_tests.rs`, Modelltests in `model.rs`

**Interfaces:**
- Produces:
  - `Group { …, parent_id: Option<GroupId> (serde default), tags: Vec<String> (serde default) }`, `Host { …, tags: Vec<String>, os: Option<HostOs> }` (beide serde default, damit ältere Datensätze lesbar bleiben).
  - `#[serde(rename_all = "snake_case")] enum HostOs { Linux, Debian, Ubuntu, Fedora, RedHat, CentOs, Rocky, Alma, Arch, Alpine, OpenSuse, Raspbian, FreeBsd, OpenBsd, MacOs, Windows, Other }`.
  - `resolve(host: &Host, chain: &[&Group]) -> Result<ResolvedHost, ResolveError>`: `chain[0]` ist die Gruppe des Hosts, dann aufwärts; ein Wert kommt vom Host, sonst von der nächsten Gruppe in der Kette, die ihn setzt. `effective_endpoint(host, chain)` ebenso. `ResolveError::WrongGroup`, wenn `chain[0]` nicht die Gruppe des Hosts ist oder ein Glied nicht das Elternteil des vorigen.
  - `group_chain(groups: &[Group], start: GroupId) -> Vec<Group>`: von `start` aufwärts, bricht bei einem Zyklus oder nach 64 Ebenen ab (gibt dann nur die bis dahin besuchten zurück).
  - Schema 5: `groups.parent_id TEXT REFERENCES groups(id) ON DELETE CASCADE`, `groups.tags TEXT NOT NULL DEFAULT '[]'`, `hosts.tags TEXT NOT NULL DEFAULT '[]'`, `hosts.os TEXT`.
  - `upsert_group` prüft `parent_id`: Elternteil muss existieren und darf nicht die Gruppe selbst oder eine ihrer Untergruppen sein, sonst neue Variante `StoreError::GroupCycle`. Eine Untergruppe übernimmt den Ort ihres Elternteils (wie ein Host den seiner Gruppe); `set_location` einer Gruppe trägt Untergruppen und deren Hosts mit; eine Untergruppe kann nicht allein verschoben werden.
  - `subgroups(id) -> Vec<GroupId>` (alle Ebenen, ohne `id`).
  - Tags werden beim Speichern getrimmt, leere entfernt, doppelte (ohne Groß und Klein) zusammengelegt, höchstens 32 je Eintrag mit höchstens 64 Zeichen.

- [ ] **Step 1: Tests schreiben:** `migration_from_v4_keeps_data`, `three_levels_inherit` (Adresse an der obersten Gruppe, Benutzer an der mittleren, Port am Host: alle drei kommen an), `nearest_group_wins`, `group_into_own_subgroup_is_refused` (`GroupCycle`), `subgroup_follows_parent_location`, `deleting_a_group_deletes_subgroups_and_their_hosts`, `group_chain_stops_on_a_cycle` (Zyklus direkt per SQL angelegt), `tags_are_normalized`, `old_host_json_without_tags_reads`.
- [ ] **Step 2–4:** rot, umsetzen, `cargo test -p sshcraft-core` grün; Aufrufer von `resolve` und `effective_endpoint` in `app/src-tauri/src/sessions.rs` und `commands/data.rs` auf die Kette umstellen.
- [ ] **Step 5: Gegenprobe:** Zyklusprüfung in `upsert_group` entfernen, `group_into_own_subgroup_is_refused` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: Untergruppen, Tags und Host-Symbol (Schema 5)`

---

### Task 2: Untergruppen im Sync

**Files:**
- Modify: `crates/sshcraft-sync/src/engine.rs`, `src/items.rs`, `crates/sshcraft-core/src/share_store.rs`
- Test: `crates/sshcraft-sync/tests/push.rs`, `tests/pull.rs`

**Interfaces:**
- Produces:
  - `record_group_hosts(store, group_id, now)` reiht Hosts aller Ebenen und alle Untergruppen ein (Umzug, Löschen).
  - `apply` legt eine Gruppe mit `parent_id` an; fehlt das Elternteil noch, greift der vorhandene Wiederholungsweg (Cursor bleibt davor). Ein Datensatz, der einen Zyklus ergäbe, wird abgelehnt (`report.rejected`, kein Wiederholen).
  - `effective_area` folgt der Kette: eigener Bereich, sonst der der Gruppe, sonst der der Elterngruppe usw. (Tiefe höchstens 64).

- [ ] **Step 1: Tests schreiben:** `subgroups_reach_other_devices` (drei Ebenen, zweites Gerät hat danach Baum und Vererbung), `child_before_parent_is_applied_later` (Datensatz der Untergruppe kommt vor dem des Elternteils), `cycle_from_the_server_is_refused` (zwei Geräte hängen gleichzeitig A unter B und B unter A; nach dem Abgleich gibt es keinen Zyklus, eine Änderung steht in `rejected`, beide Geräte haben denselben Baum), `moving_a_group_takes_subgroups_along` (lokal nach Server: alle Ebenen haben den Ort und sind eingereiht).
- [ ] **Step 2–4:** rot, umsetzen, `cargo test -p sshcraft-sync` grün.
- [ ] **Step 5: Gegenprobe:** Zyklusprüfung beim Anwenden entfernen, `cycle_from_the_server_is_refused` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-sync: Untergruppen`

---

### Task 3: Teilen mit Untergruppen

**Files:**
- Modify: `crates/sshcraft-sync/src/shares.rs`, `src/engine.rs`, `app/src-tauri/src/sync.rs` (`share_rights`)
- Test: `crates/sshcraft-sync/tests/shares.rs`, `app/src-tauri/src/sync_tests.rs`

**Interfaces:**
- Produces:
  - `GroupAll` nimmt Untergruppen und deren Hosts mit (über `effective_area`, ohne eigene Bereichszeilen); eine später angelegte Untergruppe ebenso.
  - `Selection` bietet die Hosts aller Ebenen an und legt die Kopien flach in die Kopie der geteilten Gruppe. **[Claudes Entscheidung]** Eine Kopie bekommt die Werte, die der Host von Gruppen zwischen sich und der geteilten Gruppe erbt, fest eingetragen, damit sie beim Empfänger gleich verbindet. Kostet, falls falsch: Empfänger sehen die Zwischenebenen nicht.
  - Rechte: eine Gruppe in eine empfangene Gruppe hängen ist Hinzufügen (nur Schreibrecht und "alle Hosts"), eine empfangene Untergruppe herausnehmen ist Entfernen; `share_rights` bekommt `Change::SaveGroup` mit Elternwechsel, `refusal` behandelt Gruppen in Gruppen wie Hosts.
  - Eine Gruppe, die selbst eine Freigabe "alle Hosts" ist, kann nicht in eine andere geteilte Gruppe gehängt werden (`ShareEndFirst`).

- [ ] **Step 1: Tests schreiben:** `group_share_takes_subgroups`, `later_subgroup_goes_along`, `selection_copies_inherit_intermediate_settings`, `reader_cannot_add_a_subgroup`, App-Test `moving_a_group_into_a_received_group_is_refused`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Gegenprobe:** Ketten-Rückfall in `effective_area` auf eine Ebene kürzen, `group_share_takes_subgroups` muss rot werden.
- [ ] **Step 6: Commit** `Teilen mit Untergruppen`

---

### Task 4: Gemeinsamer Import

**Files:**
- Create: `crates/sshcraft-core/src/import/mod.rs`
- Modify: `crates/sshcraft-core/src/sshconfig.rs`, `src/lib.rs`, `app/src-tauri/src/commands/sshconfig.rs`
- Test: `crates/sshcraft-core/src/import/tests.rs`

**Interfaces:**
- Produces:
  - `ImportCandidate { name, group_path: Vec<String>, settings: ConnectionSettings, tags: Vec<String>, os: Option<HostOs>, key_note: Option<KeyNote>, warnings: Vec<ImportWarning> }` (aus `sshconfig` hierher verschoben, dort neu exportiert). `ImportWarning` bekommt `PasswordNotImported`, `BadLine { line: usize }`, `BadPort`, `Unsupported { what: String }`.
  - `ImportPlan { candidates: Vec<ImportCandidate>, skipped: Vec<String> }` für die Vorschau.
  - `apply_import(store, candidates, location: Option<ServerId>) -> Result<ImportOutcome { created: usize, duplicates: usize }, StoreError>`: legt fehlende Gruppen entlang `group_path` an (gleicher Name auf gleicher Ebene wird wiederverwendet), überspringt einen Host, den es mit gleichem Namen, Adresse, Port und Benutzer in derselben Gruppe schon gibt.
  - Gemeinsame Prüfung: Port 1 bis 65535, sonst Warnung `BadPort` und Port leer; Namen höchstens 200 Zeichen; Steuerzeichen werden entfernt.

- [ ] **Step 1: Tests schreiben:** `group_paths_create_nested_groups`, `existing_groups_are_reused`, `same_host_twice_is_one_host`, `bad_port_is_dropped_with_a_warning`, und die vorhandenen `sshconfig`-Tests bleiben grün.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Gegenprobe:** Duplikatprüfung entfernen, `same_host_twice_is_one_host` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: gemeinsamer Import`

---

### Task 5: `known_hosts` und Schlüsseldateien

**Files:**
- Create: `crates/sshcraft-core/src/import/knownhosts.rs`, `import/keyfiles.rs`
- Modify: `Cargo.toml` (Workspace: `ssh-key` mit `ppk`, gleiche Fassung wie russh), `crates/sshcraft-core/src/vault.rs` (`parse_private_key` erkennt PPK)
- Test: `crates/sshcraft-core/src/import/tests.rs`

**Interfaces:**
- Produces:
  - `knownhosts::parse(text) -> KnownHostsImport { entries: Vec<KnownEntry { address, port, fingerprint }>, candidates: Vec<ImportCandidate>, skipped: Vec<String> }`: `host`, `[host]:port`, mehrere Namen mit Komma; gehashte Namen (`|1|…`) und Zeilen mit `@cert-authority` oder `@revoked` werden übersprungen und gezählt; Fingerprint im selben Format wie bei der Verbindung (`SHA256:…`).
  - `knownhosts::apply(store, entries) -> KnownOutcome { added, same, conflicts: Vec<(String, u16)> }`: ein abweichender, schon gespeicherter Fingerprint wird **nicht** überschrieben und gemeldet.
  - `keyfiles::scan(dir) -> Vec<KeyFile { path, name, algorithm, fingerprint, encrypted, format: OpenSsh|Pem|Ppk }>`: nur Dateien, die als privater Schlüssel lesbar sind; `.pub`, `known_hosts`, `config` und Dateien über 64 KiB werden nicht gelesen.
  - `keyfiles::import(vault, store, path, passphrase) -> KeyInfo`.

- [ ] **Step 1: Tests schreiben:** `plain_and_bracketed_hosts`, `hashed_names_are_skipped_and_counted`, `known_fingerprint_is_not_overwritten`, `garbage_lines_are_reported`, `scan_finds_openssh_pem_and_ppk_keys` (Testschlüssel als Dateien im Tempdir erzeugt), `encrypted_key_needs_the_passphrase`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Gegenprobe:** in `knownhosts::apply` überschreiben statt melden, `known_fingerprint_is_not_overwritten` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: Import aus known_hosts und Schlüsseldateien`

---

### Task 6: CSV

**Files:**
- Create: `crates/sshcraft-core/src/import/csv.rs`
- Test: `crates/sshcraft-core/src/import/tests.rs`

**Interfaces:**
- Produces: `csv::parse(text) -> ImportPlan`. Spalten nach Kopfzeile, ohne Groß und Klein: `name`/`label`, `group`/`groups` (Pfad mit `/`), `tags` (getrennt mit `;` oder `,`), `address`/`hostname`/`hostname/ip`/`host`, `port`, `username`/`user`, `os`; unbekannte Spalten werden ignoriert, `protocol` ungleich `ssh` überspringt die Zeile. Trennzeichen `,` oder `;` (aus der Kopfzeile erkannt). Gleiche Spalten wie der Export aus Task 9, damit ein Export wieder importierbar ist.

- [ ] **Step 1: Tests schreiben:** `own_export_round_trips` (Kopfzeile aus Task 9 vorweggenommen als Konstante `CSV_HEADER`), `semicolon_separated`, `termius_style_columns`, `missing_address_skips_the_row`, `quoted_fields_with_separators`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Commit** `sshcraft-core: Import aus CSV`

---

### Task 7: PuTTY

**Files:**
- Create: `crates/sshcraft-core/src/import/putty.rs`, `app/src-tauri/src/commands/import.rs` (Registry-Zugriff nur unter Windows)
- Test: `crates/sshcraft-core/src/import/tests.rs`

**Interfaces:**
- Produces:
  - `putty::Session { name, values: BTreeMap<String, PuttyValue> }`, `putty::parse_reg(text) -> Vec<Session>` für eine `.reg`-Datei (UTF-16 LE mit BOM oder UTF-8; Schlüssel `HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\<Name, URL-kodiert>`; `"Name"="Text"` und `"Name"=dword:0000xxxx`).
  - `putty::candidates(sessions) -> ImportPlan`: nur `Protocol` = `ssh`; `HostName` (ein `user@host` darin wird zerlegt), `PortNumber`, `UserName`; `PublicKeyFile` wird `KeyNote::File(path)`; `Default Settings` wird übersprungen.
  - App: `putty_sessions() -> ImportPlan` liest unter Windows die Registry (`winreg`), sonst leer.

- [ ] **Step 1: Tests schreiben:** `reg_file_utf16_is_read`, `url_encoded_session_names`, `non_ssh_sessions_are_skipped`, `user_at_host_is_split`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Commit** `Import aus PuTTY`

---

### Task 8: MobaXterm und SecureCRT

**Files:**
- Create: `crates/sshcraft-core/src/import/mobaxterm.rs`, `import/securecrt.rs`
- Test: `crates/sshcraft-core/src/import/tests.rs`

**Interfaces:**
- Produces:
  - `mobaxterm::parse(ini_text) -> ImportPlan`: Abschnitte `[Bookmarks]`, `[Bookmarks_1]` …, `SubRep=` ist der Gruppenpfad (`\` getrennt), jede andere Zeile `Name=#109#0%host%port%user%…` ist eine SSH-Sitzung (Typ 109 bei `#…#`-Kennung, `0` am zweiten Platz); andere Typen werden übersprungen; gespeicherte Passwörter werden nie gelesen (`PasswordNotImported`, wenn `[Passwords]` vorkommt).
  - `securecrt::parse_dir(root) -> ImportPlan`: rekursiv alle `.ini` unter `Sessions`, Ordner sind der Gruppenpfad, `__FolderData__.ini` und `Default.ini` werden übersprungen; `S:"Hostname"`, `S:"Username"`, `D:"[SSH2] Port"` (hex), `S:"Protocol Name"` muss `SSH2` sein; `S:"Identity Filename V2"` wird `KeyNote::File`; Passwort-Felder werden nicht gelesen.

- [ ] **Step 1: Tests schreiben:** `mobaxterm_bookmarks_with_folders`, `mobaxterm_non_ssh_skipped`, `mobaxterm_passwords_are_not_imported`, `securecrt_tree_becomes_groups`, `securecrt_hex_port`, `truncated_files_do_not_abort`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Commit** `sshcraft-core: Import aus MobaXterm und SecureCRT`

---

### Task 9: Export

**Files:**
- Create: `crates/sshcraft-core/src/export.rs`
- Test: `crates/sshcraft-core/src/export_tests.rs`

**Interfaces:**
- Produces:
  - `pub const CSV_HEADER: &str = "name,group,tags,address,port,username,key,os"`.
  - `export_csv(store, filter: ExportFilter) -> String` und `export_json(store, filter) -> String` (JSON: `{ "format": "sshcraft-hosts", "version": 1, "groups": [{ id, name, parent_id, tags, defaults }], "hosts": [{ id, name, group_id, tags, os, settings }] }`; Schlüssel als `{ "name", "fingerprint" }` oder `{ "agent_fingerprint" }`, nie mehr).
  - `ExportFilter { location: Option<Option<ServerId>>, group: Option<GroupId> }` (alles, nur ein Ort, nur ein Teilbaum).

- [ ] **Step 1: Tests schreiben:** `export_never_contains_secrets` (Host mit gespeichertem Schlüssel; der exportierte Text enthält weder `PRIVATE KEY` noch einen Teil der versiegelten Bytes, nur Name und Fingerprint), `csv_round_trips_through_the_importer`, `json_keeps_the_tree`, `filter_by_subtree`.
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Gegenprobe:** im JSON für einen gespeicherten Schlüssel den ganzen `KeyRecord` (mit `sealed_secret`) statt Name und Fingerprint ausgeben, `export_never_contains_secrets` muss rot werden.
- [ ] **Step 6: Commit** `sshcraft-core: Export als CSV und JSON ohne Geheimnisse`

---

### Task 10: Betriebssystem erkennen

**Files:**
- Modify: `crates/sshcraft-ssh/src/lib.rs`, `src/testing.rs`, `app/src-tauri/src/sessions.rs`
- Create: `crates/sshcraft-core/src/os_detect.rs`
- Test: `crates/sshcraft-ssh/tests/session.rs`, `crates/sshcraft-core/src/os_detect.rs` (Tests im Modul)

**Interfaces:**
- Produces:
  - `Session::exec(&self, command: &str, limit: usize) -> Result<Vec<u8>, SshError>` (eigener Kanal, höchstens `limit` Bytes, Zeitlimit 5 s).
  - `os_detect::from_output(text) -> Option<HostOs>`: `ID=`/`ID_LIKE=` aus `/etc/os-release` zuerst, sonst `uname -s` (`Darwin`, `FreeBSD`, `OpenBSD`, `Linux`).
  - App: nach der ersten erfolgreichen Verbindung eines Hosts ohne `os` einmal `cat /etc/os-release 2>/dev/null; uname -s` ausführen, Ergebnis speichern und wie eine Änderung einreihen (nur wenn der Host nicht aus einer Freigabe mit Leserecht stammt; dann nur lokal). Fehlschläge werden still ignoriert.

- [ ] **Step 1: Tests schreiben:** `debian_ubuntu_alpine_rocky_from_os_release`, `id_like_falls_back`, `darwin_and_freebsd_from_uname`, `unknown_is_none`, SSH-Test `exec_returns_output` (TestServer antwortet auf Exec mit festem Text).
- [ ] **Step 2–4:** rot, umsetzen.
- [ ] **Step 5: Commit** `Betriebssystem eines Hosts erkennen`

---

### Task 11: App-Befehle für Import und Export

**Files:**
- Create: `app/src-tauri/src/commands/import.rs`, `commands/export.rs`
- Modify: `app/src-tauri/Cargo.toml` (`tauri-plugin-dialog`), `src/lib.rs`, `capabilities/*.json`, `errors.rs`, `app/package.json`

**Interfaces:**
- Produces: `import_preview(source: ImportSource) -> ImportPlanView` mit `ImportSource = SshConfig | KnownHosts { path } | KeyFiles { dir } | Csv { path } | PuttyRegistry | PuttyReg { path } | MobaXterm { path } | SecureCrt { dir }` (Pfadvorgaben je Plattform: `~/.ssh/known_hosts`, `~/.ssh`, `%APPDATA%\VanDyke\Config\Sessions`, `%USERPROFILE%\Documents\MobaXterm\MobaXterm.ini`); `import_apply(candidates, location) -> ImportOutcome` (eingereiht für den Server des Orts); `import_known_hosts(path) -> KnownOutcome`; `import_key_file(path, passphrase) -> KeyInfo`; `export_hosts(format: Csv|Json, filter, path)` schreibt die Datei, die der Nutzer im Speichern-Dialog gewählt hat. Neue `ErrorCode`s: `ImportUnreadable`, `ImportTooLarge` (Dateien über 10 MiB), `GroupCycle`.

- [ ] **Step 1: Tests schreiben:** Rust-Tests in `commands/import.rs` für `import_apply` mit Ort (eingereiht) und ohne (nicht eingereiht), `too_large_file_is_refused`.
- [ ] **Step 2–4:** rot, umsetzen; `cargo test -p sshcraft-app` grün.
- [ ] **Step 5: Commit** `App: Befehle für Import und Export`

---

### Task 12: Oberfläche

**Files:**
- Create: `app/src/components/ImportDialog.tsx`, `ExportDialog.tsx`, `HostIcon.tsx`, `TagInput.tsx`, Tests `ImportDialog.test.tsx`, `TagInput.test.tsx`, `hostsTree.test.ts`
- Modify: `app/src/pages/HostsPage.tsx`, `hostsView.ts`, `components/GroupForm.tsx`, `HostForm.tsx`, `SshConfigImport.tsx` (geht im Import-Dialog auf), `shell/AppShell.tsx`, `palette/*`, `api.ts`, `i18n/en.json`, `de.json`, `styles.css`

**Interfaces:**
- Produces:
  - Hosts-Seite: oben die Gruppen der aktuellen Ebene, darunter ihre Hosts; Doppelklick auf eine Gruppe öffnet sie (Brotkrumen-Leiste zurück); die Suche sucht über alle Ebenen und zeigt den Gruppenpfad; Tag-Chips filtern (mehrere: alle müssen passen); jede Karte zeigt das Symbol (`HostIcon`: Distributionslogo aus `simple-icons`, sonst ein neutrales Server-Symbol).
  - `GroupForm`: Elterngruppe (Baum, ohne die Gruppe selbst und ihre Untergruppen), Tags. `HostForm`: Gruppe als Pfad, Tags, Symbol (automatisch oder von Hand).
  - Import-Dialog: Quelle wählen, Pfad vorbelegt oder über den Datei-Dialog, Vorschau mit Warnungen und Häkchen je Eintrag, Ziel-Ort, Ergebnis "angelegt / schon vorhanden / übersprungen"; bei `known_hosts` zusätzlich die Zahl übernommener Fingerprints und die Konflikte; bei Schlüsseldateien Passphrase je verschlüsseltem Schlüssel.
  - Export-Dialog: Format, Umfang (alles, ein Ort, eine Gruppe), Hinweis "ohne Schlüssel und Passwörter", Speichern-Dialog.
  - Befehlspalette: "Importieren …", "Exportieren …".

- [ ] **Step 1: Tests schreiben:** `hostsTree.test.ts` (Pfad einer Gruppe, Hosts einer Ebene, Suche über Ebenen, Tag-Filter mit zwei Tags), `TagInput.test.tsx` (Enter und Komma legen an, Doppelte nicht), `ImportDialog.test.tsx` (Vorschau zeigt Warnungen, abgewählte Einträge gehen nicht in `import_apply`, Ergebnis wird angezeigt); der Wächter gegen feste Texte bleibt grün.
- [ ] **Step 2–4:** rot, umsetzen; `npm test`, `npm run typecheck` grün.
- [ ] **Step 5: Commit** `Oberfläche: Untergruppen, Tags, Symbole, Import und Export`

---

### Task 13: Doku und Gesamtprüfung

**Files:**
- Create: `docs/hosts.md` (Ordnung mit Untergruppen und Vererbung, Tags, Symbole, jede Importquelle mit Pfad und Grenzen, Export und was nicht drin ist)
- Modify: `README.md` (Zeile in der Doku-Tabelle), `docs/sync.md` (Untergruppen und Teilen)

- [ ] **Step 1:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, in `app/` `npm test` und `npm run typecheck`: grün.
- [ ] **Step 2: Paketstand:** `cargo update --dry-run --verbose` und `npm outdated`; Anheben, was geht, Zurückgehaltenes mit Grund ins Ledger.
- [ ] **Step 3: Installer bauen** (`npm run tauri build`).
- [ ] **Step 4: Commit** `Doku: Hosts und Ordnung`

---

## Zuordnung Spec zu Task

| Spec | Task |
|---|---|
| 2 Gruppen mit Untergruppen, Vererbung über alle Ebenen | 1, 2, 12 |
| 2 Tags | 1, 12 |
| 2 Host-Symbole nach Betriebssystem oder Distribution | 1, 10, 12 |
| 2 Import `~/.ssh/config` (vorhanden) | 4 (umgestellt), 12 |
| 2 Import `known_hosts`, Schlüsseldateien | 5, 11, 12 |
| 2 Import CSV | 6 |
| 2 Import PuTTY (Registry unter Windows, sonst `.reg`) | 7, 11 |
| 2 Import MobaXterm, SecureCRT | 8 |
| 2 Export CSV und JSON ohne Geheimnisse | 9, 11, 12 |
| 4 Ort: Untergruppe folgt dem Ort der Gruppe | 1, 2 |
| 8 Teilen: Gruppe "alle Hosts" samt Untergruppen | 3 |

Nicht in diesem Teilplan: Passwörter und Identitäten aus Importquellen (Teilplan 9), Sprung-Hosts aus `ProxyJump` (Teilplan 9, der `ssh/config`-Import meldet sie weiter als Warnung).
