# SSHCraft Teilplan R5: Oberfläche II Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Die zweite Hälfte der Oberfläche: Workspaces mit geteilten Bereichen, Broadcast und Vorlagen, Portweiterleitung, Snippets mit Verlauf und Vervollständigung, Sitzungsprotokolle, Sync, Teilen und Geräte, die Rechte der KI-Agenten mit ihrer Rückfrage, dazu die offenen Punkte aus R4.

**Architecture:**
- **Wie R4:** Seiten lesen den Cache (`Data`) und handeln über den Runner; ein Dialog sendet einen Befehl zur Zeit (`forms.rs`), ein spätes Ergebnis trifft nur seinen Dialog. Jede neue Seite steht in der Seitenleiste (Spec 7.2: Hosts, Schlüssel, Snippets, Portweiterleitung, Protokolle, Einstellungen).
- **Bereiche (Panes) statt Sitzung je Tab.** Ein Tab hält bis zu 16 Bereiche, jeder Bereich ist eine Sitzung (`SessionTab` aus R4 wird `Pane`) oder eine kleine Neuer-Tab-Seite. Broadcast, Statuspunkte, Rec-Marke und Vervollständigung hängen am Bereich.
- **Die Engine bleibt, wie sie ist**, bis auf: Sitzungs-Id in den Rückfragen, ein Wecker für den Terminal-Stream, OSC 7 im `term`-Crate, Scrollback als Einstellung. Alles andere sind vorhandene Befehle aus R2.

**Tech Stack:** wie R4 (egui/eframe 0.37 Fork, craft-libs `cebc847`, egui_kittest). Neu: `qrcode` (MIT OR Apache-2.0, neueste Fassung, ohne `image`-Feature) für den Kontoschlüssel als QR-Code.

**Spec:** `docs/superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`, Abschnitte 7.2, 7.3, 9 (Rückfrage im Fenster), 13 (Zeile R5), 16.1 (AccessKit), 17 (Regeln 1, 3, 4, 5, 6, 10, 11, 13). Verhalten nach `docs/workspaces.md`, `docs/forwarding.md`, `docs/snippets.md`, `docs/session-logs.md`, `docs/sync.md`, `docs/agents.md`, `docs/shortcuts.md`. Offene Punkte aus R4: `docs/gaps.md` 1, 4, 4a (Wecker), 4c (Scrollback), 4d (Ligaturen); Ruling 38 (Sitzungs-Id in Rückfragen).

## Global Constraints

- **Arbeitsbaum** `G:\Claude\worktrees\sshcraft-r5`, Zweig `r5-interface`, abgezweigt von `r4-interface`; ist PR #11 gemergt, wird `origin/main` hineingemergt. Pfad nur in diesem Plan.
- **Sprache:** Code, Kommentare, Doku, Commits englisch, Aufgaben-Id je Commit (`R5.3: workspaces`). Kein KI-Hinweis. Oberflächentexte englisch im Code, deutsch in `i18n/de.rs` (der Test `every_text_has_german` gilt weiter). Plan deutsch.
- **Alles aus R4 gilt:** nie abstürzen, kein Blockieren, Farben nur aus Tokens, Icons im Code, Beschriftungen laufen nie über (`elided_label`, `visible`), AccessKit-Beschriftung für alles Bedienbare, Geheimnisse in `secret_field`, ein Dialog sendet über `Forms::start`/`done`.
- **Grenzen aus der Doku sind Regeln** (16 Bereiche, 100 Vorlagen, 50 Regeln je Host, 256 Verbindungen, 1000 Snippets, 1000 Verlaufszeilen, 500 Marken, 100 000 Zeilen im Betrachter, 200 Suchtreffer). Die Engine prüft sie schon; die Oberfläche zeigt sie, bevor sie sendet.
- **Kein Shell-Text aus fremden Daten (Spec 17.1):** Verzeichnis und Befehl einer Vorlage gehen als eine geprüfte Zeile in die Sitzung, das Verzeichnis in einfachen Anführungszeichen mit `'\''` für jedes `'`.
- **Agenten erreichen nichts davon:** die Agent-Rechte, ihre Rückfrage und `prompt.agent` bleiben Befehle des Fensters (die Engine erzwingt das, Tests aus R2).
- **Gegenproben** für jeden Test einer Regel, eines Rechts oder eines Datenwegs (Broadcast ohne Terminal-Antworten, Agent-Rückfrage, Sicherheitsnummer beim Teilen, Verlauf ohne Passwörter, Export-Pfade).
- **Jede Aufgabe endet mit grünem `cargo xtask ci` in PowerShell.**

## Review Focus

1. **Broadcast an einen Bereich, der gerade eine Rückfrage zeigt oder beendet ist**: Erwartet: Tippen erreicht nur Bereiche mit verbundener Sitzung, nie eine Passwort-Rückfrage eines anderen Bereichs, nie die automatischen Antworten des Terminals (Test `broadcast_skips_ended_panes_and_terminal_replies`, Task 4).
2. **Die Agent-Rückfrage kommt, während der Benutzer in einem Terminal tippt**: Erwartet: nichts Getipptes beantwortet sie, die Knöpfe warten 400 ms, nach 2 Minuten zählt sie als Ablehnung (Test `typing_never_answers_an_agent_question`, Task 12).
3. **Ein Passwort, das in einer Shell-Abfrage getippt wird**: Erwartet: es kommt nie in den Verlauf und nie in die Vervollständigung (Test `input_after_a_password_question_is_never_kept`, Task 8).
4. **Eine Vorlage mit Verzeichnis `'; rm -rf ~ #`**: Erwartet: die Sitzung bekommt `cd -- ''\''; rm -rf ~ #'` als eine Zeile, nichts davon läuft als eigener Befehl (Test `a_template_directory_is_quoted_once`, Task 5).
5. **Teilen, nachdem sich der Schlüssel des Empfängers geändert hat**: Erwartet: Warnung mit alter und neuer Sicherheitsnummer, kein Teilen bis zur neuen Bestätigung (Test `a_changed_recipient_key_stops_the_share`, Task 11).

---

### Task 1: Engine-Nachträge aus R4

**Files:**
- Modify: `crates/engine/src/prompts.rs` (`HostKeyPrompt`, `AuthPrompt` je `pub session: Option<SessionId>`), `crates/engine/src/sessions.rs` (Id durch `run` → `connect_session` → `StoreVerifier` und einen Auth-Adapter je Verbindung), `crates/engine/src/terminal.rs` (Wecker), `crates/engine/src/lib.rs` (`Session::set_terminal_waker(waker: Arc<dyn Fn() + Send + Sync>)`), `crates/engine/src/settings.rs` (`Terminal::scrollback: usize`, Vorgabe 10 000, Bereich 1000 bis 100 000; `Appearance::ligatures: bool`, Vorgabe true)
- Modify: `crates/ui-egui/src/prompts.rs` (Rückfrage nennt ihren Tab und holt ihn nach vorn, wenn der Benutzer gerade nicht tippt), `crates/ui-egui/src/session_tab.rs` (Wecker statt 60-Hz-Abfrage; `TermConfig::scrollback` aus der Einstellung), `crates/ui-egui/src/terminal/layout.rs` (ohne Ligaturen jede schmale Zelle als eigener Abschnitt), `crates/ui-egui/src/pages/settings.rs`
- Test: `crates/engine/src/sessions_tests.rs`, `crates/ui-egui/tests/session.rs`, `crates/ui-egui/src/terminal/layout_tests.rs`, `crates/engine/src/settings/tests.rs`

**Interfaces:**
- Produces: `HostKeyPrompt.session`, `AuthPrompt.session` (None für Weiterleitungen, Agent-Sitzungen und `key.install`); `PromptView.session: Option<SessionId>`; `Session::set_terminal_waker`; `EngineSettings.terminal.scrollback`, `.appearance.ligatures`.

- [ ] **Step 1: Tests:** `prompts_name_their_session` (Engine: Host-Schlüssel- und Anmelde-Rückfrage einer `session.open`-Sitzung tragen deren Id), `a_question_shows_which_tab_it_belongs_to` (UI: zwei Tabs zum selben Host, die Rückfrage des zweiten nennt dessen Titel und ein Klick auf "Show tab" zeigt ihn), `output_wakes_the_window` (der Wecker wird nach `data` und `status` gerufen), `scrollback_is_clamped` (0 → 1000, 10⁷ → 100 000), `without_ligatures_each_cell_is_its_own_run` (Layout: `->` ergibt zwei Abschnitte).
- [ ] **Step 2:** rot sehen; **Step 3:** umsetzen; die 60-Hz-Abfrage fällt weg (`docs/gaps.md` 4a erledigt). **Step 4:** grün, Gegenprobe: Wecker nie gerufen → `output_wakes_the_window` rot. **Step 5:** Commit `R5.1: session ids in questions, terminal waker, scrollback and ligature settings`.

### Task 2: Seitenleiste und Seitenrahmen

**Files:**
- Modify: `crates/ui-egui/src/state.rs` (`Page::{Snippets, Forwarding, Logs}`), `crates/ui-egui/src/shell.rs`, `crates/ui-egui/src/commands.rs` (`ui.page.snippets`, `ui.page.forwarding`, `ui.page.logs` mit Aktionen `snippetsPage`, `forwardingPage`, `logsPage` in `SHORTCUT_ACTIONS`), `crates/ui-egui/src/widgets/icons.rs` (Icons Snippet, Forward, Log, Broadcast, Rec)
- Create: leere Seiten `pages/{snippets,forwarding,logs}.rs` mit Überschrift

- [ ] **Step 1: Test** `the_sidebar_has_every_page` (Hosts, Keys, Snippets, Port forwarding, Logs, Settings, in dieser Reihenfolge, jede führt zu ihrer Seite). **Steps 2–4**, Commit `R5.2: sidebar pages`.

### Task 3: Bereiche (Panes) in einem Tab

**Files:**
- Create: `crates/ui-egui/src/panes.rs` (Raster, Größen, Fokus), `crates/ui-egui/tests/panes.rs`
- Modify: `state.rs` (`TabKind::Panes { panes: Vec<PaneState>, focused: usize, layout: Layout, col_sizes: Vec<f32>, row_sizes: Vec<f32>, broadcast: bool }`, `PaneState { kind: PaneKind::New | PaneKind::Session { session } , title }`), `session_tab.rs` (wird `pane.rs`: eine Sitzung je Bereich), `app.rs`, `commands.rs` (`ui.pane.split` Ctrl+Shift+D, `ui.pane.close` Ctrl+Shift+X, `ui.pane.next` Ctrl+Shift+PageDown, `ui.pane.previous` Ctrl+Shift+PageUp, `ui.pane.layout` Ctrl+Shift+M; Aktionen `splitPane`, `closePane`, `nextPane`, `prevPane`, `toggleLayout` aus der Tauri-Liste)

**Interfaces:**
- Produces: `panes::grid(count: usize) -> (cols: usize, rows: usize)` (1→1×1, 2→2×1, 3 und 4→2×2, 5 und 6→3×2, bis 16→4×4; der letzte Bereich einer Zeile reicht bis zum Rand); `SshcraftApp::split()`, `close_pane()`, `focus_pane(step: isize)`; `MAX_PANES = 16`.

- [ ] **Step 1: Tests:** `the_grid_is_near_square` (Tabelle oben), `split_opens_a_new_pane_page_and_stops_at_16`, `closing_the_last_pane_closes_the_tab` (und die Sitzung), `focus_layout_shows_one_pane_large_and_lists_the_others`, `dragging_a_divider_resizes` (Spaltenbreite bleibt ≥ 10 %), `dragging_a_title_bar_onto_another_pane_swaps_them`, `move_to_own_tab_and_merging_keep_the_sessions` (keine neue Verbindung: Sitzungs-Ids gleich), `a_tab_with_one_pane_looks_like_a_plain_tab` (kein Titelbalken).
- [ ] **Steps 2–4**, Gegenprobe: Schließen ohne `session.close` → `closing_the_last_pane…` rot. Commit `R5.3: panes`.

### Task 4: Broadcast und Statuspunkte

**Files:**
- Modify: `pane.rs`, `shell.rs` (Tab zeigt den dringendsten Punkt und die Antenne), `commands.rs` (`ui.pane.broadcast` Ctrl+Shift+B, Aktion `toggleBroadcast`)
- Create: `crates/ui-egui/src/status.rs` (`Dot::{Error, Waiting, Unseen, Connected, Connecting, Ended}` mit Rangfolge in dieser Reihenfolge; `waiting_line(line: &str) -> bool` für Passwort, Passphrase, PIN, Token, Code, yes/no in EN, DE, FR, ES, PT)

**Interfaces:**
- Produces: `status::Dot`, `status::waiting_line`, `Pane::dot() -> Dot`; Broadcast schickt `input` des fokussierten Bereichs an alle Bereiche mit `Connected`-Status, `Notice::Reply` nie.

- [ ] **Step 1: Tests:** `broadcast_skips_ended_panes_and_terminal_replies` (Review Focus 1: drei Bereiche, einer beendet, ein Geräteattribut-Antwort im fokussierten: nur getippter Text kommt in den beiden verbundenen an), `broadcast_is_off_for_new_tabs`, `broadcast_marks_panes_red_and_the_tab`, `dots_rank_error_waiting_unseen` (Tabelle), `looking_at_a_pane_clears_unseen_and_waiting_not_error`, `waiting_line_knows_password_questions` (`"Password:"`, `"Passwort für root:"`, `"Enter passphrase for key"`, `"Mot de passe :"`, `"Contraseña:"`, `"Senha:"`, `"(yes/no)?"`; nicht `"password reset done"`).
- [ ] **Steps 2–4**, Gegenprobe: `Notice::Reply` mitgesendet → erster Test rot. Commit `R5.4: broadcast and status dots`.

### Task 5: Vorlagen (Workspace-Templates) und OSC 7

**Files:**
- Modify: `crates/term/src/filter.rs` (OSC 7 `file://host/path` → `Notice::WorkingDirectory(String)`, nur einzeilig, höchstens 1000 Zeichen, prozentdekodiert, sonst verworfen), `crates/term/src/lib.rs`
- Create: `crates/ui-egui/src/pages/templates.rs` (Speichern-Dialog, Liste auf der Neuer-Tab-Seite, Palette "Workspace: name"), `crates/ui-egui/tests/templates.rs`

**Interfaces:**
- Consumes: `workspace.list|save|delete` (Template `{ id, name, layout, col_sizes, row_sizes, panes: [{ target, title, cwd?, command? }] }`).
- Produces: `templates::start_line(cwd: Option<&str>, command: Option<&str>, posix: bool) -> Option<String>` (`cd -- '<dir>' && <command>`; nur Befehl bei serieller Leitung; `None` wenn beides fehlt).

- [ ] **Step 1: Tests:** `osc7_sets_the_working_directory` (und `file://h/a%20b` → `/a b`; über 1000 Zeichen oder mit Zeilenumbruch → nichts), `a_template_directory_is_quoted_once` (Review Focus 4), `a_template_round_trips` (speichern, öffnen: gleiche Anordnung, Größen, Ziele, Titel), `a_deleted_host_is_left_out_with_a_notice`, `templates_stop_at_100_and_16_panes`, `a_local_cwd_must_be_local_and_absolute` (UNC und relativ → Heimatverzeichnis), `broadcast_is_off_in_an_opened_template`.
- [ ] **Steps 2–4**, Gegenprobe: Anführungszeichen nicht maskiert → Review-Focus-Test rot. Commit `R5.5: workspace templates and OSC 7`.

### Task 6: Portweiterleitung

**Files:**
- Create: `crates/ui-egui/src/pages/forwarding.rs` (Liste aller Regeln aller Hosts mit Zustand; Assistent "New rule": Art, Host, Adressen und Ports, Zusammenfassung mit der `ssh`-Option; Warnung bei anderen Adressen als Loopback; "Without a rule"), `crates/ui-egui/tests/forwarding.rs`
- Modify: `app.rs` (`EngineEvent::ForwardStatus` → `Data.forwards: HashMap<ForwardId, ForwardStatus>`), `data.rs` (`forward.status` beim Start)

- [ ] **Step 1: Tests:** `a_local_rule_is_created_started_and_stopped` (Testserver aus `test_host`; Zustand Running mit `bound_port`, dann Stopped), `the_wizard_warns_about_0_0_0_0`, `a_running_rule_cannot_be_edited`, `rules_stop_at_50_per_host`, `the_summary_shows_the_ssh_option` (`-L 127.0.0.1:8080:db:5432`), `refused_connections_show_in_the_row`, `reachable_rules_are_marked` (auch über Sync gekommene).
- [ ] **Steps 2–4**, Gegenprobe: Warnung entfernt → `the_wizard_warns…` rot. Commit `R5.6: port forwarding`.

### Task 7: Snippets und Seitenleiste am Terminal

**Files:**
- Create: `crates/ui-egui/src/pages/snippets.rs` (Pakete, "Without package", Formular, Ausführen-Dialog mit Hosts und offenen Bereichen, "Run" und "Paste only"), `crates/ui-egui/src/terminal_sidebar.rs` (Ctrl+Shift+S: Snippets und Verlauf des fokussierten Bereichs, eine Suche, Verlauf löschen), `crates/ui-egui/tests/snippets.rs`
- Modify: `commands.rs` (`ui.terminal.sidebar` Ctrl+Shift+S, Aktion neu `snippetSidebar`), `palette.rs` ("Snippet: name")

**Interfaces:**
- Produces: `snippets::paste_bytes(content: &str, bracketed: bool, run: bool) -> Vec<u8>` (mit Klammer-Einfügen, wenn der Bereich es meldet; ohne: Zeilen einzeln, Tabs als Leerzeichen; `run` hängt `\r` an).

- [ ] **Step 1: Tests:** `a_snippet_is_saved_and_listed_by_package`, `running_on_new_hosts_opens_one_pane_each_and_types_once` (16 je Tab), `running_on_open_panes_types_at_once`, `paste_only_leaves_it_at_the_prompt`, `multi_line_uses_bracketed_paste_when_the_shell_has_it`, `snippet_limits` (Inhalt 10 000 Zeichen, keine Steuerzeichen außer Zeilenumbruch und Tab), `the_sidebar_pastes_into_all_panes_when_broadcast_is_on`.
- [ ] **Steps 2–4**, Gegenprobe: Steuerzeichen durchgelassen → `snippet_limits` rot. Commit `R5.7: snippets and the terminal sidebar`.

### Task 8: Verlauf und Vervollständigung

**Files:**
- Create: `crates/ui-egui/src/history.rs` (Zeilenerfassung je Bereich: entscheidet beim ersten Zeichen anhand der Cursorzeile, ob es eine Shell-Eingabe ist; Ausschlüsse aus `docs/snippets.md`), `crates/ui-egui/src/completion.rs` (Vorschläge: Verlauf, dann Snippets, dann Pfade über `session.list_dir`, 10 s Cache; Overlay über dem Terminal), `crates/ui-egui/tests/history.rs`, `crates/ui-egui/tests/completion.rs`

**Interfaces:**
- Consumes: `history.record|list|clear`, `session.list_dir`, `status::waiting_line`.
- Produces: `history::Capture::{key(&mut self, …), line_done(&mut self) -> Option<String>}`; `completion::suggest(line: &str, history: &[String], snippets: &[SnippetRow]) -> Vec<Suggestion>`.

- [ ] **Step 1: Tests:** `input_after_a_password_question_is_never_kept` (Review Focus 3, alle Sprachen aus Task 4), `lines_starting_with_a_space_are_not_kept`, `repl_and_fullscreen_input_is_not_kept` (mysql, psql, sqlite, python, irb, `>`-Fortsetzung, Alternativbildschirm), `edited_lines_are_not_kept` (Tab, Pfeile, Ctrl+R), `multi_line_pastes_and_broadcast_are_not_kept`, `history_off_clears_it`, `suggestions_start_at_two_characters_and_never_take_a_key` (Enter ohne Auswahl geht an die Shell), `down_selects_tab_takes_the_rest_only`, `a_snippet_by_name_replaces_the_line`, `paths_need_a_slash_and_are_cached_10_s`.
- [ ] **Steps 2–4**, Gegenprobe: Passwortregel aus → Review-Focus-Test rot. Commit `R5.8: history and completion`.

### Task 9: Sitzungsprotokolle

**Files:**
- Create: `crates/ui-egui/src/pages/logs.rs` (Liste mit Host, Beginn, Dauer, Größe, Ort; Filter; Suche ab 2 Zeichen, höchstens 200 Treffer; Löschen der Auswahl oder alles Gezeigten; Export .txt oder .log über craft-dialogs), `crates/ui-egui/src/log_viewer.rs` (schreibgeschütztes `term`-Terminal mit Marken: Lesezeichen bis 100 Zeichen, Kommentar 1 bis 2000, 500 je Protokoll, 100 000 Zeilen), `crates/ui-egui/tests/logs.rs`
- Modify: `pane.rs` (Rec-Marke unten rechts, Ctrl+Shift+L Lesezeichen mit Uhrzeit, Aktion `bookmark`), `pages/settings.rs` (Aufzeichnen, Aufbewahrung 0/7/30/90/365), `pages/host_form.rs` (Aufzeichnen: Von der Gruppe/Ja/Nein)

- [ ] **Step 1: Tests:** `a_recorded_session_is_listed_and_plays_back`, `rec_shows_while_recording`, `ctrl_shift_l_adds_a_bookmark`, `marks_are_added_jumped_to_and_removed`, `search_opens_the_log_at_the_hit`, `a_recording_log_cannot_be_deleted`, `export_writes_text_where_the_user_chose` (Dateidialog-Override), `a_long_log_says_its_beginning_is_missing`.
- [ ] **Steps 2–4**, Gegenprobe: Löschen eines laufenden Protokolls erlaubt → Test rot. Commit `R5.9: session logs`.

### Task 10: Sync und Geräte

**Files:**
- Create: `crates/ui-egui/src/pages/sync.rs` (Einstellungen > Sync: Server mit Zustand aus `SyncStatus`, Schalter Sync und Schlüssel, Protokoll-Sync; Konto anlegen und beitreten; Kontoschlüssel einmal mit QR-Code, nie gespeichert; Geräte mit Widerrufen; "Add device"; Liste der Verbindungen, die SSHCraft selbst aufbaut, Spec 17.6), `crates/ui-egui/src/widgets/qr.rs` (QR-Code aus `qrcode`, mit dem Painter gezeichnet), `crates/ui-egui/tests/sync.rs`
- Modify: Kontextmenüs von Host, Gruppe und Schlüssel ("Move to": Gerät oder Server, `item.move`), `app.rs` (`SyncStatus`, `SyncWarning` → Meldung)

- [ ] **Step 1: Tests** (gegen `sshcraft-server-lib` im Prozess wie die Sync-Tests aus R2): `an_account_is_created_and_its_key_shown_once` (Schlüssel danach nirgends im Zustand), `a_second_device_joins_with_the_key`, `revoking_a_device_warns_about_copied_keys`, `the_recovery_device_cannot_be_revoked`, `a_group_moves_with_its_hosts`, `status_dots_follow_sync_status`, `the_connections_list_names_every_server`.
- [ ] **Steps 2–4**, Gegenprobe: Kontoschlüssel im Zustand behalten → erster Test rot. Commit `R5.10: sync and devices`.

### Task 11: Teilen

**Files:**
- Create: `crates/ui-egui/src/pages/sharing.rs` (Teilen-Dialog: Empfänger per E-Mail, Sicherheitsnummer in sechs Fünfergruppen mit Haken, Art Gruppe/Auswahl/Host, Rollen Lesen oder Lesen und Ändern, Schlüssel nur auf Wunsch; Einstellungen > Sync > Shares: eigene und erhaltene, Empfänger entfernen mit Warnung, Teilen beenden), `crates/ui-egui/tests/sharing.rs`

- [ ] **Step 1: Tests:** `a_group_is_shared_after_the_safety_number_is_ticked`, `a_changed_recipient_key_stops_the_share` (Review Focus 5), `agent_keys_never_go_along`, `removing_a_recipient_warns_when_keys_were_shared`, `ending_a_share_brings_the_group_back`.
- [ ] **Steps 2–4**, Gegenprobe: Haken nicht verlangt → erster Test rot. Commit `R5.11: sharing`.

### Task 12: KI-Agenten: Rechte und Rückfrage

**Files:**
- Create: `crates/ui-egui/src/pages/agents.rs` (Einstellungen > AI agents: Stufe je Kategorie, versteckte Hosts und Gruppen mit "Show again", Protokoll mit Löschen), `crates/ui-egui/tests/agents.rs`
- Modify: `prompts.rs` (`PromptKind::Agent`: Agent, Aktion, Ziel, Befehl; bei Hosts jedes geänderte Feld alt und neu; Warnung bei mehrzeiligen, über 300 Zeichen langen oder verdeckte Zeichen tragenden Befehlen; dieselbe 400-ms-Sperre; "Allow"/"Deny"), Kontextmenüs von Host und Gruppe ("Hide from agents")

- [ ] **Step 1: Tests** (Agent-Aufrufe über `Session::execute` mit `Caller::Agent`): `an_agent_question_names_agent_action_and_target`, `typing_never_answers_an_agent_question` (Review Focus 2), `a_long_command_carries_a_warning`, `levels_are_set_per_category`, `a_hidden_host_is_listed_and_shown_again`, `the_log_lists_outcomes_and_clears`.
- [ ] **Steps 2–4**, Gegenprobe: Sperre bei Agent-Rückfragen aus → zweiter Test rot. Commit `R5.12: agent rights and questions`.

### Task 13: Doku und Abschluss

**Files:** `docs/architecture.md`, `docs/decisions.md`, `docs/gaps.md` (1, 4, 4a, 4c, 4d erledigt), `docs/shortcuts.md`, `docs/security.md`, `ROADMAP.md`, `docs/roadmap.md`, `README.md`, `AGENTS.md`

- [ ] **Step 1:** Doku nachziehen. **Step 2:** Snapshots der neuen Seiten erzeugen (`snapshot --page snippets|forwarding|logs`) und selbst ansehen. **Step 3:** Abschluss-Review (opus), alle Befunde beheben, auch die kleinen, jeder mit Test, der vorher rot war. **Step 4:** ci, PR mit Auto-Merge (Merge-Commit), Liste für die Probe von Hand.
