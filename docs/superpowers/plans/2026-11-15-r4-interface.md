# SSHCraft Teilplan R4: Oberfläche I Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Die Desktop-App kommt zurück: `apps/sshcraft` startet ein egui-Fenster auf der Engine mit Seitenleiste, Tabs, Befehlspalette, Tokens und Themes, Englisch und Deutsch (AUTO), eigenen Tastenkürzeln, den Seiten für Hosts, Gruppen, Schlüssel, Identitäten, Import und Export, und Verbinden in Terminal-Tabs mit allen Rückfragen.

**Architecture:**
- **Dünne Oberfläche (Spec 7.1).** `crates/ui-egui` liest den Zustand der Engine und handelt über `App::run(id, params, then)`. Ausgeführt wird jeder Befehl als Task auf der Runtime der Engine; das Ergebnis kommt über einen Kanal zurück und wird im nächsten Frame angewandt, nie blockiert ein Frame auf die Engine. Listen (Hosts, Gruppen, Schlüssel, Identitäten) liegen in einem Cache, der bei `EngineEvent::DataChanged` neu geholt wird.
- **Zustand in `state.rs` (serde)** wie die Familie (texturecraft): Seite, offene Tabs, Auswahl, Filter, Palette; flüchtiges mit `#[serde(skip)]`. Der Control-Channel (R6) liest ihn; R4 baut kein Setzen von außen.
- **Zwei Befehlsquellen in der Palette:** die Engine-Befehle aus `command_specs()` (nur die für den Benutzer sinnvollen, mit `menu`-Pfad) und die Oberflächen-Befehle `ui.*` (Tabs, Palette, Seiten). Tastenkürzel über `craft-keymap` aus der Tabelle in `docs/shortcuts.md`, eigene Belegung in `EngineSettings::shortcuts`.
- **Bausteine aus craft-libs** (eine Revision, `cebc847`): `craft-ui` (Theme-Helfer `WidgetPalette`, Tabs, Fokus), `craft-keymap` (`egui`), `craft-window` (`egui`, Titelleiste), `craft-dialogs` (Dateidialoge, die nie blockieren), `craft-quit` (`egui`). Nicht in R4: `craft-menu` (Menüleiste nur auf macOS, R7), `craft-i18n` (seine Kataloge liegen im Repository `storytold/craft-i18n`; SSHCraft hält Englisch und Deutsch im eigenen Repository, siehe Task 3).

**Tech Stack:** Rust 1.99, egui/eframe 0.37 (Fork `storytold/egui`, wie R3), craft-libs `cebc847`, egui_kittest für die Tests, `sshcraft-engine` mit Feature `testing` in den Tests.

**Spec:** `docs/superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`, Abschnitte 2 (Ziel 3), 7.1, 7.2, 7.4, 8, 13 (Zeile R4), 16.1 (AccessKit R4), 17 (Regeln 2, 3, 9, 11). Verhalten der Seiten nach `docs/hosts.md`, `docs/keys.md`, `docs/connections.md`, `docs/shortcuts.md` (die englische Doku der Tauri-Fassung, das Verhalten bleibt). Offene Punkte aus R3 (`docs/gaps.md` 4a, 4b, 4c) und aus R2 (Meldung bei laufender Tauri-Fassung).

## Voraussetzungen

Nichts Neues auf dem PC. Netz zu github.com beim ersten Bau (craft-libs-Crates aus dem bekannten Checkout, egui-Fork wie R3).

## Global Constraints

- **Arbeitsbaum** `G:\Claude\worktrees\sshcraft-r4`, Zweig `r4-interface`, abgezweigt von `r3-terminal`; ist PR #8 gemergt, wird auf `main` umgesetzt (`git rebase --onto origin/main r3-terminal r4-interface`). Pfad nur in diesem Plan.
- **Sprache:** Code, Kommentare, Doku, Commits englisch, Aufgaben-Id je Commit (`R4.3: English and German`). Kein KI-Hinweis, kein `Co-Authored-By`. Oberflächentexte englisch im Code, deutsch in der Tabelle aus Task 3. Plan deutsch.
- **Schichten:** `ui-egui` bleibt L6; `apps/sshcraft` (Paket `sshcraft`, Binärname `sshcraft`) ist ausgenommen. Nur `ui-egui` und `apps/*` hängen an egui, eframe, winit oder rfd.
- **Nie abstürzen:** kein `unwrap`/`expect`/`panic` außerhalb von Tests. Eingaben aus Formularen werden vor `run` geprüft, die Engine prüft ein zweites Mal. Ein Fehler der Engine wird eine Meldung (Toast), nie ein Absturz.
- **Kein Blockieren:** Kein Frame wartet auf die Engine (`block_on` nur in `main` vor dem Start des Fensters). Dateidialoge über `craft-dialogs`.
- **Farben nur aus `theme::Tokens`** (Spec 7.1), jedes Paar hell und dunkel geprüft: Text auf Fläche 7:1, schwacher Text 4,5:1, Text auf Auswahl 4,5:1, Fehler und Warnung 3:1, Akzent 3:1 (Symbole), wie texturecraft. Kein Farbwert in einem Widget fest verdrahtet.
- **Icons im Code gezeichnet** (`widgets/icons.rs`, Formen mit dem Painter): keine Bilddatei, kein Download, nichts von Termius.
- **Beschriftungen laufen nie über** (`elided_label` mit Tooltip, Spec 7.1).
- **AccessKit:** Jedes bedienbare Element hat eine Beschriftung, die Tests finden es darüber (`get_by_label`).
- **Daten bleiben gleich** (Schema 11, Schlüsselbund-Dienst, Datenordner). Neue Geräte-Einstellungen nur in `config.toml` mit Vorgabe.
- **Gegenproben** für jeden Test einer Regel, eines Rechts oder eines Datenwegs (OSC-52-Zustimmung, Link-Öffnen, Geheimnisse in Formularen, Tastenkürzel-Konflikte, Übersetzungs-Vollständigkeit).
- **Jede Aufgabe endet mit grünem `cargo xtask ci` in PowerShell.**
- **Anhalten** nur bei Lizenz außerhalb der Liste, Sicherheitsmeldung ohne Fix, Geheimnissen, schwer Umkehrbarem.

## Review Focus

1. **Die Engine antwortet langsam oder gar nicht** (Server hängt bei `session.open`, Sync blockiert): Erwartet: das Fenster bleibt bedienbar, die Aktion zeigt "läuft", ein späteres Ergebnis trifft nicht die falsche Seite (Test `a_slow_command_does_not_block_a_frame`, Task 1).
2. **Eine Rückfrage kommt, während ein Dialog offen ist oder ein anderer Tab vorne liegt** (Host-Schlüssel, Anmeldung): Erwartet: sie wird gezeigt, sobald möglich, und gehört sichtbar zu ihrer Sitzung; ein Klick für die vorige Rückfrage beantwortet nicht die neue (Test `a_new_prompt_is_not_answered_by_the_click_for_the_old_one`, Task 9).
3. **Ein Host-Name, Tag oder Gruppenname mit Steuer-, Bidi- oder sehr vielen Zeichen** (aus Import oder Sync): Erwartet: gekürzt mit Tooltip, Bidi-Zeichen sichtbar gemacht, kein Layout-Bruch (Test `hostile_names_are_shown_safely`, Task 6).
4. **Ein Programm auf dem Server schreibt die Zwischenablage (OSC 52) ohne Zustimmung für diesen Host**: Erwartet: nichts in der Zwischenablage, eine Frage mit Einmal/Immer/Nein; "Immer" gilt nur für diesen Host (Test `clipboard_writes_need_consent_per_host`, Task 9).
5. **Zwei Fenster-Starts** (die App läuft schon, oder die Tauri-Fassung): Erwartet: der zweite Start zeigt eine Meldung und beendet sich, ohne Datenbank oder Tresor zu öffnen (Test `a_second_start_explains_and_stops`, Task 1).

---

### Task 1: App-Gerüst, Runner, Ereignisse, Start

**Files:**
- Create: `apps/sshcraft/{Cargo.toml, src/main.rs}`, `crates/ui-egui/src/{app.rs, runner.rs, state.rs, data.rs, toasts.rs, testing.rs}`, `crates/ui-egui/tests/app.rs`
- Modify: `Cargo.toml` (craft-ui, craft-keymap, craft-window, craft-dialogs, craft-quit aus `cebc847`, Features wie die Familie; `sshcraft` App), `crates/ui-egui/{Cargo.toml, src/lib.rs}` (Feature `testing`), `xtask/src/layers.rs` falls nötig

**Interfaces:**
- Produces:
  - `pub struct SshcraftApp` mit `SshcraftApp::new(session: Session, ctx: &egui::Context) -> Self` und `pub fn frame(&mut self, ui: &mut egui::Ui)` (eine Frame-Runde: Ergebnisse anwenden, Ereignisse der Engine lesen, zeichnen)
  - `runner.rs`: `pub struct Runner`, `Runner::run(&self, id: &str, params: Value, then: Then)`, `pub type Then = Box<dyn FnOnce(&mut SshcraftApp, Result<Value, EngineError>) + Send>`, `Runner::take_done(&self) -> Vec<(Then, Result<Value, EngineError>)>`, `Runner::busy(&self) -> usize`
  - `data.rs`: `pub struct Data { hosts: Vec<Host>, groups: Vec<Group>, keys: Vec<KeyInfo>, identities: Vec<Identity>, settings: EngineSettings, .. }`, `Data::refresh(app, what: Refresh)` mit `bitflags Refresh { HOSTS, GROUPS, KEYS, IDENTITIES, SETTINGS }` aus `DataChanged`
  - `state.rs`: `#[derive(Serialize, Deserialize, Default)] pub struct UiState { pub page: Page, pub tabs: Vec<TabState>, pub active_tab: Option<usize>, pub hosts: HostsView, #[serde(skip)] pub palette: PaletteState, .. }`, `pub enum Page { NewTab, Hosts, Keys, Identities, Settings(SettingsPage), Session(TabId) }`
  - `toasts.rs`: `Toasts::error(text)`, `Toasts::info(text)`, gezeichnet unten rechts, Fehler bleiben bis zum Schließen
  - `testing.rs` (Feature `testing`): `pub fn app_harness(dir: &Path) -> (Harness<'static, SshcraftApp>, Session)` mit Engine aus `SessionConfig::for_tests`
  - `apps/sshcraft/src/main.rs`: Runtime, `Session::open` auf `sshcraft_platform::data_dir()`, bei `DataInUse` oder `LegacyRunning` ein kleines Fenster mit Erklärung und "Close" (nichts geöffnet), sonst `eframe::run_native` mit Titel "SSHCraft", Mindestgröße 720×480, Icon aus `icons::app_icon() -> egui::IconData` (im Code gezeichnet)

- [ ] **Step 1: Tests schreiben** in `crates/ui-egui/tests/app.rs`: `the_app_starts_on_the_hosts_page` (Harness, `get_by_label("Hosts")` ist ausgewählt), `a_slow_command_does_not_block_a_frame` (ein Testbefehl der Engine, der 2 s schläft, über `Runner::run`; zehn Frames vergehen unter 200 ms; das Ergebnis kommt danach an und landet beim richtigen `Then`), `an_engine_error_becomes_a_toast` (`host.delete` mit unbekannter Id → Toast mit dem Fehlertext, kein Absturz), `data_follows_data_changed` (Host über die Engine angelegt → nach Frames in `Data::hosts`), und in `apps/sshcraft` `a_second_start_explains_and_stops` (zweite `Session::open` auf denselben Ordner → `StartError::DataInUse` mit Text, kein Fenster der App).
- [ ] **Step 2: rot sehen.** `cargo test -p sshcraft-ui-egui --test app` Expected: Kompilierfehler.
- [ ] **Step 3: Umsetzen.** Testbefehl für den langsamen Lauf: im Engine-Feature `testing` `test.sleep { ms }` neben `test.echo` (Liste der Befehls-Ids in `tests/it/command_ids.txt` nicht berühren, Testbefehle stehen dort nicht). `Then` läuft im Frame-Thread mit `&mut SshcraftApp`.
- [ ] **Step 4: grün, Gegenprobe** (Runner blockiert mit `block_on` → `a_slow_command…` rot), `cargo xtask ci`, Commit `R4.1: app shell, runner and start`.

### Task 2: Tokens, Themes, Schriften

**Files:**
- Create: `crates/ui-egui/src/theme/{mod.rs, tokens.rs, apply.rs, contrast_tests.rs}`, `crates/ui-egui/build.rs` (CRAFT_FONTS_DIR wie texturecraft: `OUT_DIR/craft_fonts.rs`, leer ohne Ordner), `crates/ui-egui/src/fonts.rs`
- Modify: `app.rs` (Theme bei Änderung anwenden), `terminal/palette.rs` (Terminal-Palette aus den Tokens: `TermPalette::for_theme(dark: bool)`)

**Interfaces:**
- Produces: `pub struct Tokens { bg, panel, sidebar, row, row_hover, row_selected, separator, text, text_dim, text_strong, accent, accent_strong, accent_dim, field, field_border, button, button_border, danger, warning, success, radius: u8 }` (alle `Color32`), `Tokens::dark()`, `Tokens::light()`, `Tokens::get(ctx) -> Tokens`, `theme::apply(ctx, &EngineSettings)` (System folgt `ctx.system_theme()`), `fonts::install(ctx, &EngineSettings)` (Inter aus craft-fonts wenn eingebaut, sonst egui; Terminal-Schrift aus R3)

- [ ] **Step 1: Tests:** `contrast_tests.rs`: `every_theme_reads` (Paare und Grenzen aus den Global Constraints, für `dark()` und `light()`); `tests/app.rs`: `the_theme_follows_the_setting` (`settings.save` mit `app_theme: "light"` → `ctx.style().visuals.dark_mode == false`).
- [ ] **Step 2: rot**, **Step 3:** eigene Farben (keine von Termius), Akzent ein Grün nahe `#00E676` für dunkel und `#00873E` für hell geprüft; `craft_ui::theme::WidgetPalette::apply_to` für die Widget-Zustände.
- [ ] **Step 4: grün, Gegenprobe** (`text_dim` dunkel heller machen → `every_theme_reads` rot), ci, Commit `R4.2: design tokens, themes and fonts`.

### Task 3: Englisch und Deutsch

**Files:**
- Create: `crates/ui-egui/src/i18n/{mod.rs, de.rs}`, `crates/ui-egui/tests/i18n.rs`

**Interfaces:**
- Produces: `pub fn tr(text: &'static str) -> &'static str`, `pub fn trf(template: &'static str, args: &[(&str, &str)]) -> String` (`{name}`-Platzhalter), `pub fn set_language(lang: Lang)`, `pub enum Lang { En, De }`, `Lang::resolve(setting: LanguageSetting) -> Lang` (AUTO: `sys_locale`, `de*` → De, sonst En); `de.rs`: `pub fn translate(text: &str) -> Option<&'static str>` (ein `match` je Text, wie gridcraft)
- Ruling (im Ledger beim Umsetzen): eigene Tabelle statt craft-i18n, weil dessen Kataloge im Repository `storytold/craft-i18n` liegen und SSHCraft dort keine Texte ablegt; die Schnittstelle (`tr`, `trf`) folgt craft-i18n, ein späterer Umzug ändert nur `i18n/`.

- [ ] **Step 1: Tests** in `tests/i18n.rs`: `every_text_has_german` (sammelt alle Literale in `tr("…")` und `trf("…"` unter `crates/ui-egui/src/**/*.rs` und prüft `de::translate` für jedes; nennt die fehlenden), `placeholders_match` (jede deutsche Vorlage hat dieselben `{…}` wie die englische), `auto_follows_the_system` (`Lang::resolve(Auto)` mit `de-DE` → De, `fr-FR` → En; die Funktion nimmt die Locale als Parameter, `resolve` holt sie).
- [ ] **Step 2: rot**, **Step 3:** umsetzen; alle Texte der folgenden Tasks gehen durch `tr`.
- [ ] **Step 4: grün, Gegenprobe** (einen deutschen Eintrag entfernen → `every_text_has_german` rot), ci, Commit `R4.3: English and German`.

### Task 4: Befehle, Tastenkürzel, Befehlspalette

**Files:**
- Create: `crates/ui-egui/src/{commands.rs, keymap.rs, palette.rs}`, `crates/ui-egui/tests/{palette.rs, keymap.rs}`

**Interfaces:**
- Produces:
  - `commands.rs`: `pub struct UiCommand { id: &'static str, label: &'static str, default: &'static [&'static str], scope: Scope }`, `pub fn ui_commands() -> &'static [UiCommand]` mit den Ids aus `docs/shortcuts.md`: `ui.palette`, `ui.tab.new`, `ui.tab.close`, `ui.tab.next`, `ui.tab.previous`, `ui.page.hosts`, `ui.page.keys`, `ui.page.settings`, `ui.terminal.copy`, `ui.terminal.paste` (R5 bringt Split, Broadcast, Snippets, Lesezeichen); `pub enum Scope { App, OutsideTerminal, Terminal }`; `pub fn palette_entries(app) -> Vec<Entry>` (Oberflächen-Befehle plus Engine-Befehle mit `menu`-Pfad, die ohne Parameter Sinn ergeben oder einen Dialog öffnen)
  - `keymap.rs`: `pub struct Bindings` über `craft_keymap::Keymap`, `Bindings::from_settings(&Shortcuts) -> Bindings`, `Bindings::dispatch(ctx, in_terminal: bool) -> Vec<&'static str>`; im Terminal nur Ctrl+Shift (ohne Alt) und Ctrl+Tab (`docs/shortcuts.md`)
  - `palette.rs`: `PaletteState { open, query, selected }`, Bewertung wie pdfcraft (übersetzte Beschriftung, dann englische, dann Id), deaktivierte zuletzt, höchstens 12, Pfeile/Enter/Escape

- [ ] **Step 1: Tests:** `tests/keymap.rs`: `defaults_match_the_shortcut_table` (jede Zeile aus `docs/shortcuts.md` ist belegt), `inside_a_terminal_only_ctrl_shift_and_ctrl_tab_are_taken` (Ctrl+K im Terminal → kein Befehl, Ctrl+Shift+P → `ui.palette`), `a_taken_combination_is_refused_with_its_owner` (eigene Belegung, die schon vergeben ist → Fehler mit dem Namen der Aktion), `a_shortcut_needs_a_modifier_or_a_function_key`; `tests/palette.rs`: `the_palette_finds_by_german_and_english_label`, `arrows_enter_escape`, `disabled_entries_come_last`.
- [ ] **Step 2: rot**, **Step 3:** umsetzen; eigene Belegung wird über `settings.save` (`shortcuts.bindings`) gespeichert.
- [ ] **Step 4: grün, Gegenprobe** (Terminal-Filter aus → zweiter Keymap-Test rot), ci, Commit `R4.4: commands, shortcuts and the command palette`.

### Task 5: Fenster, Seitenleiste, Tabs, Titelleiste

**Files:**
- Create: `crates/ui-egui/src/{shell.rs, sidebar.rs, tabs.rs, new_tab.rs, widgets/mod.rs, widgets/icons.rs, widgets/elided.rs}`, `crates/ui-egui/tests/shell.rs`
- Modify: `apps/sshcraft/src/main.rs` (craft-window `policy::decide_for` und `apply`, `craft_quit` in `logic`)

**Interfaces:**
- Produces: Seitenleiste mit Hosts, Keys (Schlüssel und Identitäten), Settings; die Einträge für R5 (Snippets, Port forwarding, Logs) erscheinen nicht vor R5. Tab-Leiste: Neuer-Tab-Seite (zuletzt verbunden aus `host.recent`, Suche, lokale Shell, serieller Port), Statuspunkt je Sitzung, Schließen per Mittelklick und Kreuz. `widgets::elided_label(ui, text, max_width)`, `widgets::icons::{draw(painter, Icon, rect, color)}` mit `Icon::{Host, Group, Key, Identity, Settings, Terminal, Plus, Search, Close, Folder, Serial, Warning}`, `icons::app_icon() -> egui::IconData`.

- [ ] **Step 1: Tests** `tests/shell.rs`: `the_sidebar_switches_pages`, `tabs_open_and_close_by_shortcut` (Ctrl+Shift+T öffnet die Neuer-Tab-Seite, Ctrl+Shift+W schließt), `ctrl_tab_cycles_tabs`, `the_new_tab_page_lists_recent_hosts`, `labels_never_overflow` (sehr langer Host-Name in der Tab-Leiste → Breite begrenzt, Tooltip mit vollem Namen).
- [ ] **Step 2–4** wie oben; Gegenprobe für `labels_never_overflow` (Kürzen aus → rot); Commit `R4.5: window, sidebar, tabs and title bar`.

### Task 6: Hosts und Gruppen

**Files:**
- Create: `crates/ui-egui/src/pages/{mod.rs, hosts.rs, host_form.rs, group_form.rs}`, `crates/ui-egui/tests/hosts.rs`

**Interfaces:**
- Consumes: `host.list`, `group.list`, `host.save`, `group.save`, `host.delete`, `group.delete`, `item.move`, `host.recent`, `host.info`, `identity.list`, `key.list`, `password.set`, `password.clear`, `host.forget_key`, `settings.save` (`appearance.host_view`).
- Produces: Ansicht Raster oder Liste (`appearance.host_view`), eine Ebene je Gruppe mit Pfad darüber (Doppelklick öffnet), Tag-Chips (UND), Suche über Name, Adresse, Benutzer, Gruppenpfad, Tags; Kontextmenü (Verbinden, Bearbeiten, Duplizieren, Verschieben nach…, Host-Schlüssel vergessen, Löschen mit Bestätigung); Enter oder Doppelklick verbindet. Formular mit Feldern aus `docs/connections.md`: Name, Gruppe, Adresse, Port, Benutzer, Anmeldung (Schlüssel, Identität, Passwort), Sprung-Hosts, Proxy, Agent-Weiterleitung, Umgebungsvariablen, Terminal-Typ, Symbol, Tags; geerbte Werte sichtbar als Platzhalter mit der Gruppe, aus der sie kommen. Ein Passwortfeld zeigt nie einen gespeicherten Wert, nur "gespeichert" und "ändern".

- [ ] **Step 1: Tests:** `a_host_is_created_edited_and_deleted`, `groups_open_by_double_click_and_the_path_leads_back`, `tags_filter_with_and`, `search_covers_the_group_path`, `inherited_values_show_their_group`, `a_stored_password_is_never_shown` (Formular nach `password.set` → das Feld ist leer, die AccessKit-Werte enthalten das Passwort nicht), `a_group_cannot_move_into_itself` (Verschieben-Liste bietet sich selbst und Untergruppen nicht an), `hostile_names_are_shown_safely` (Review Focus 3: Name mit `\u{202e}`, Steuerzeichen und 5000 Zeichen → angezeigt gekürzt, Bidi als `⟨U+202E⟩`).
- [ ] **Step 2–4**, Gegenproben: Passwort ins Feld laden → `a_stored_password…` rot; Bidi-Anzeige aus → `hostile_names…` rot. Commit `R4.6: hosts and groups`.

### Task 7: Schlüssel und Identitäten

**Files:**
- Create: `crates/ui-egui/src/pages/{keys.rs, key_dialogs.rs, identities.rs}`, `crates/ui-egui/tests/keys.rs`

**Interfaces:**
- Consumes: `key.list`, `key.generate`, `key.import`, `key.delete`, `key.protect`, `key.remember_passphrase`, `key.attach_certificate`, `key.detach_certificate`, `key.install`, `key.create_security`, `key.security_support`, `key.agent_list`, `key.passcli_status`, `key.passcli_list`, `key.passcli_import`, `vault.status`, `vault.hello`, `identity.list`, `identity.save`, `identity.delete`.
- Produces: Liste aller Arten aus `docs/keys.md` (Tresor, FIDO2, Windows Hello, Proton-Pass-Agent) mit Typ, Fingerabdruck, Zertifikat; Erzeugen (Typ Ed25519/ECDSA/RSA, Bits, Kommentar, Passphrase), Importieren (Text oder Datei über craft-dialogs), Löschen mit Bestätigung, Passphrase setzen, Zertifikat anhängen, In `authorized_keys` eintragen (Host-Auswahl), Sicherheitsschlüssel anlegen (nur wenn `key.security_support` es meldet), pass-cli-Import; öffentlicher Schlüssel kopieren. Identitäten: Benutzer plus Schlüssel oder Passwort.

- [ ] **Step 1: Tests:** `a_key_is_generated_and_listed`, `importing_shows_the_fingerprint_not_the_key`, `no_private_key_reaches_the_screen` (nach Import eines Schlüssels enthält der AccessKit-Baum weder `BEGIN OPENSSH PRIVATE KEY` noch Teile davon), `copy_public_key_copies_only_the_public_part`, `security_keys_only_when_supported`, `an_identity_uses_a_key`.
- [ ] **Step 2–4**, Gegenprobe: privaten Schlüssel ins Vorschaufeld → `no_private_key…` rot. Commit `R4.7: keys and identities`.

### Task 8: Import und Export

**Files:**
- Create: `crates/ui-egui/src/pages/{import.rs, export.rs}`, `crates/ui-egui/tests/import_export.rs`

**Interfaces:**
- Consumes: `import.defaults`, `import.preview`, `import.apply`, `import.ssh_config_preview`, `import.ssh_config_apply`, `import.known_hosts`, `import.scan_keys`, `import.key_file`, `host.export`.
- Produces: Assistent mit Quelle (die sieben aus `docs/hosts.md`), Datei über craft-dialogs oder Vorgabe aus `import.defaults`, Vorschau mit Häkchen (Einträge mit Warnung starten aus), übersprungene Zeilen aufgelistet, Ziel (Gruppe, Ort). Export: CSV oder JSON, alles oder eine Gruppe, Datei über craft-dialogs.

- [ ] **Step 1: Tests** (Dateidialoge über `craft_dialogs::set_override`): `a_csv_import_creates_only_checked_entries`, `entries_with_a_warning_start_unchecked`, `an_export_writes_no_secret` (Export nach `password.set` → Datei enthält das Passwort nicht, `has_password: true`), `a_dialog_answered_later_still_reaches_its_page`.
- [ ] **Step 2–4**, Gegenprobe für `entries_with_a_warning…`. Commit `R4.8: import and export`.

### Task 9: Verbinden: Sitzungs-Tabs und Rückfragen

**Files:**
- Create: `crates/ui-egui/src/{session_tab.rs, prompts.rs}`, `crates/ui-egui/tests/session.rs`
- Modify: `crates/engine/src/settings.rs` (`Terminal::clipboard_hosts: Vec<HostId>`, Vorgabe leer, Grenze 1000 Einträge; Test `clipboard_hosts_round_trip`), `docs/security.md`, `docs/gaps.md` (4a, 4b erledigt)

**Interfaces:**
- Consumes: `session.open`, `session.send`, `session.resize`, `session.close`, `prompt.list`, `prompt.host_key`, `prompt.auth`, `link.open`, `local.shells`, `serial.ports`, `serial.last`, `serial.remember`, `Session::attach_terminal`, `Pump` und `TerminalView` aus R3.
- Produces: Tab je Sitzung mit `TerminalView`, `Pump` je Frame, Antworten des Terminals (`Notice::Reply`) zurück an die Sitzung, Titel aus `Notice::Title`, Statusleiste (verbunden, verbindet neu mit Versuch und Sekunden, beendet mit Grund und "Neu verbinden"), Schließen des Tabs schließt die Sitzung. Rückfragen als Fenster über dem Tab ihrer Sitzung: Host-Schlüssel (neu: Fingerabdruck und Annehmen/Abbrechen; geändert: Warnung, nur Abbrechen, Spec Abschnitt 10), Anmeldung (Passwort, Passphrase, keyboard-interactive mit Feldern, Echo nach Vorgabe des Servers). Die Antwort-Knöpfe reagieren erst nach 400 ms (wie die Tauri-Fassung). OSC 52: `Notice::ClipboardWrite` eines Hosts in `clipboard_hosts` → Zwischenablage; sonst Frage "Erlauben: Einmal / Immer für diesen Host / Nein"; bei `clipboard_osc52 = off` nie. Strg+Klick auf Link → `link.open` (die Engine prüft http/https). Ctrl+Shift+C kopiert die Auswahl, Ctrl+Shift+V fügt ein.

- [ ] **Step 1: Tests** (Test-Server aus `sshcraft_engine::testing::trusted_test_host` und ein nicht vertrauter): `connecting_opens_a_terminal_tab`, `an_unknown_host_key_is_asked_and_accepted`, `a_changed_host_key_is_never_accepted`, `a_new_prompt_is_not_answered_by_the_click_for_the_old_one` (Review Focus 2), `clipboard_writes_need_consent_per_host` (Review Focus 4: ohne Zustimmung nichts in `ctx.copied_text`, "Immer" speichert nur diesen Host), `closing_the_tab_closes_the_session`, `a_link_opens_only_through_the_engine` (Ctrl+Klick → `link.open` mit der URI; `file:`-Link → Fehler-Toast, nichts geöffnet), `ctrl_shift_c_and_v_copy_and_paste`.
- [ ] **Step 2–4**, Gegenproben: Zustimmung übergehen → `clipboard_writes…` rot; Verzögerung der Knöpfe aus → `a_new_prompt…` rot. Commit `R4.9: session tabs and prompts`.

### Task 10: Einstellungen, Snapshot des Fensters, Budget der leeren App

**Files:**
- Create: `crates/ui-egui/src/pages/settings.rs`, `crates/ui-egui/tests/settings.rs`, `crates/ui-egui/tests/budgets.rs` (Release)
- Modify: `crates/ui-egui/examples/snapshot.rs` (zusätzlich ganze Fenster: `--page hosts|keys|settings|newtab`, `--dark|--light`, `--language en|de`, Engine mit Beispieldaten aus `SessionConfig::for_tests`), `xtask/src/main.rs` (`perf` nimmt `-p sshcraft-ui-egui --test budgets` dazu)

**Interfaces:**
- Produces: Einstellungs-Seiten Allgemein (Sprache), Aussehen (App-Theme, Terminal-Theme, Schrift, Größe, Host-Ansicht), Terminal (Typ, OSC 52 an/aus und die Liste der erlaubten Hosts mit Entfernen, lokale Standard-Shell), Tastenkürzel (Liste aller Aktionen, Ändern wartet auf die nächste Kombination, Entfernen, Standard; Konflikte mit Name; "nur außerhalb des Terminals" sichtbar). Alles über `settings.get`/`settings.save`.

- [ ] **Step 1: Tests:** `changing_the_language_changes_the_labels`, `a_shortcut_is_changed_removed_and_reset`, `a_clipboard_host_can_be_removed`, und in `budgets.rs` (Release): `the_empty_app_stays_in_its_memory_budget` (App mit leerer Engine, 120 Frames, Zuwachs des Arbeitsspeichers ≤ 200 MiB über den Zustand vor dem ersten Frame, gemessen mit memory-stats), `a_frame_of_the_hosts_page_with_5000_hosts_takes_under_8_ms`.
- [ ] **Step 2–4**, Gegenprobe für die Tastenkürzel-Konflikte; Snapshot-Bilder für die PR-Beschreibung erzeugen und selbst ansehen; Commit `R4.10: settings, window snapshot and app budgets`.

### Task 11: Doku und Abschluss

**Files:** `docs/architecture.md` (Oberfläche, App), `docs/decisions.md`, `docs/gaps.md` (1 aktualisieren, 4a/4b/4c erledigt), `docs/shortcuts.md` (Hinweis auf R4-Stand), `ROADMAP.md`, `docs/roadmap.md`, `AGENTS.md` ("Running and looking at it": `cargo run -p sshcraft`), `README.md` (Schnellstart, falls er die App nennt)

- [ ] **Step 1:** Doku nachziehen (englisch, ohne Pfade dieses Rechners).
- [ ] **Step 2:** Abschluss-Review des ganzen Zweigs (opus), alle Befunde beheben, auch die kleinen (Moritz, 15.11.2026), jeder mit Test, der vorher rot war.
- [ ] **Step 3:** ci, Commit, Push, PR mit Snapshot-Hinweis und der Liste für die Probe von Hand (App starten, Host anlegen, verbinden, Host-Schlüssel annehmen, Sprache umstellen, Tastenkürzel ändern), CI abwarten, mergen (Merge-Commit).
