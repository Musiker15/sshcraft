# SSHCraft Teilplan 4: Oberfläche nach Termius-Vorbild und Sprachen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Die Windows-App bekommt den Aufbau nach Termius-Vorbild (Tab-Leiste mit Startseite, Seitenleiste mit Hosts, Schlüssel, Snippets, Portweiterleitung, Protokolle und Einstellungen, Befehlspalette, Neuer-Tab-Seite, Themes und Schriften), alle Texte laufen über Englisch und Deutsch mit AUTO, Rust liefert Fehlercodes statt Texten, und die 22 zurückgestellten Review-Punkte aus den Teilplänen 1 bis 3 sind erledigt.

**Architecture:** Eine kleine eigene Übersetzungsschicht (`app/src/i18n/`, flache JSON-Dateien je Sprache, React-Kontext) ohne Bibliothek. Rust gibt an der Oberfläche nur noch `ErrorCode` (snake_case) plus einen technischen `detail` aus; ein Rust-Test prüft, dass jeder Code in beiden Sprachdateien steht. Einstellungen und letzte Verbindungen liegen im vorhandenen SQLite-Store (Schema 2). Der neue Rahmen ersetzt `App.tsx`, `Sidebar.tsx` und die Tab-Leiste aus `TerminalTabs.tsx`; Terminals bleiben über Tab-Wechsel hinweg eingehängt. Die Restpunkte in `sshcraft-proto` sind unabhängig vom Rest.

**Tech Stack:** wie Teilplan 2, dazu `lucide-react` 1.72.0 (Icons, ISC), Rust `strum` 0.28.0 (Iteration über `ErrorCode`), `sys-locale` 0.3.2 (Systemsprache), `tauri-plugin-single-instance` 2.5.5 (die 3.0-Alpha gehört zu Tauri 3), `wait-timeout` in der neuesten Version (Zeitlimit für `pass-cli`).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md` (Abschnitte 2 "Funktionsumfang nach Termius-Vorbild", "Aussehen", "Sprachen", 9, 11), Fahrplan `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md`, Vorgänger `2026-11-12-02-schluesselquellen-windows.md` und `2026-11-12-03-protokoll-krypto.md`, Protokoll `docs/protocol.md`.

## Global Constraints

- Alle Regeln aus Teilplan 1 bis 3 gelten weiter (Arbeitsbaum unter `G:\Claude\worktrees\sshcraft-oberflaeche`, Zweig `feat/oberflaeche`; Code Englisch; Commits Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche; UTF-8; neueste stabile Versionen; Gegenproben für Regeln, Rechte und Datenwege).
- Sprachen: AUTO (Vorgabe), Englisch, Deutsch. AUTO nimmt die Sprache des Betriebssystems; ist sie weder Deutsch noch Englisch, gilt Englisch. Weitere Sprachen entstehen durch eine zusätzliche JSON-Datei.
- Kein Oberflächentext steht im Code. Jeder Text kommt aus `app/src/i18n/en.json` und `de.json`; Englisch ist die Rückfallsprache. In beiden Dateien kein Gedankenstrich (`—`, `–`).
- Fehler aus Rust kommen als `{ code, detail }` an. `code` wird übersetzt, `detail` ist technisch (englisch, unübersetzt) und erscheint nur aufklappbar unter "Details".
- Aussehen: Aufbau und Abläufe nach Termius, aber eigenes Logo, eigene Farben, Icons aus `lucide-react`, eigene Texte [Claudes Entscheidung, Design Abschnitt 2 "Aussehen"]. Akzentfarbe `#f5a524` (Bernstein), damit SSHCraft nicht wie Termius aussieht [Claudes Entscheidung].
- Globale Tastenkürzel nehmen der Shell keine Taste weg: im Terminal nur Kombinationen mit Strg+Umschalt oder Strg+Tab.
- Snippets, Portweiterleitung und Protokolle bekommen in diesem Teilplan nur ihren Platz in der Navigation und eine Hinweisseite; gefüllt werden sie in den Teilplänen 12 bis 14.
- Ein Schnellverbinden mit `user@host` aus dem Suchfeld gehört nicht in diesen Teilplan (kommt mit Teilplan 8).

## Review Focus

1. **Tastenkürzel im Terminal:** Strg+W, Strg+K, Strg+R, Strg+T, Strg+C, Strg+D, Strg+L, Strg+Umschalt+C, Strg+Umschalt+V und Alt+Taste erreichen die Shell unverändert. Test in Task 7.
2. **Ungewöhnliche Systemsprache bei AUTO** (`de_AT.UTF-8`, `DE`, `C`, leer, keine Angabe, `fr-FR`): Deutsch nur bei `de*`, sonst Englisch, kein Absturz. Test in Task 1.
3. **Einstellungen aus einer älteren oder neueren Fassung oder beschädigt** (fehlende Felder, unbekannter Wert in einem Feld, Schriftgröße 0 oder 500, kein JSON): nur das betroffene Feld fällt auf die Vorgabe zurück, die App startet. Test in Task 5.
4. **Wechsel auf die Startseite oder einen anderen Tab** schließt keine Sitzung und hängt kein Terminal aus; eine offene Fingerprint-Frage verschwindet mit ihrem Tab. Tests in Task 4 und Task 7.
5. **Suche mit Umlauten, Groß- und Kleinschreibung, zerlegten Zeichen (NFD) und Emojis:** "MÜNCHEN" findet "münchen", ein NFD-"München" wird gefunden. Test in Task 8.

---

## Zurückgestellte Review-Punkte und wo sie erledigt werden

| Herkunft | Punkt | Task |
|---|---|---|
| TP 1 | Fingerprint-Frage bleibt offen, wenn ihr Tab geschlossen wird | 4 |
| TP 1 | Tresor-Hinweis ist derselbe, egal welcher Fehler | 2 |
| TP 1 | Englische Fehlertexte aus Rust in der deutschen Oberfläche | 2 |
| TP 1 | Kein Schutz gegen zwei gleichzeitig laufende Instanzen | 12 |
| TP 1 | Adressauflösung doppelt (auch bei "Fingerprint vergessen") | 2 |
| TP 1 | CSP ließe sich verschärfen | 12 |
| TP 1 | Erster fehlgeschlagener Versuch heißt "Verbindung verloren" | 2 |
| TP 2 | Geschlossener Tab lässt offenen Windows-Hello-Dialog zurück | 4 |
| TP 2 | ssh-config-Vorschau ohne Proton Pass meldet "nicht in Proton Pass" | 3 |
| TP 2 | Hello-Hinweis wird nur beim ersten verbundenen Tab geprüft | 4 |
| TP 2 | Doppelte `Include`-Zeilen erzeugen doppelte React-Schlüssel | 3 |
| TP 2 | `pass-cli` meldet jeden Fehler als "nicht angemeldet" | 3 |
| TP 2 | `pass-cli`-Aufruf ohne Zeitlimit | 3 |
| TP 2 | `ProxyJump`, `ProxyCommand` und `%h` werden stillschweigend übernommen | 3 |
| TP 2 | Relative PATH-Einträge bei der Suche nach `pass-cli` | 3 |
| TP 3 | Schlüsselreste im freigegebenen Speicher bei Export, Import, `generate` | 11 |
| TP 3 | Server-Anmeldung nicht an die Server-Adresse gebunden | 11 |
| TP 3 | Fehlende Vorbedingungen in Doku und Rustdoc | 11 |
| TP 3 | Vier Tests fürs Geräteprotokoll | 11 |
| TP 3 | `known.seq as usize` schneidet auf 32 Bit ab | 11 |
| TP 1 | Langsamer Import blockiert das Fenster | schon in TP 2 erledigt |
| TP 1 | Ungenutztes `import_key_file` | schon in TP 2 erledigt |

---

## Dateistruktur

```
crates/sshcraft-core/src/
  model.rs            + effective_endpoint()
  store.rs            Schema 2: settings, recent_hosts; get/put_setting, touch_recent, recent_hosts
  sshconfig.rs        KeyNote, ImportWarning, %h-Erweiterung, plan_import(agent: Option<&[String]>)
crates/sshcraft-proto/src/
  identity.rs         Zeroizing im Export/Import/generate, sign_auth/verify_auth mit server_origin
  devicelog.rs        usize::try_from, Rustdoc-Vorbedingungen
  record.rs           Rustdoc-Vorbedingung für open
crates/sshcraft-proto/tests/devicelog.rs   + vier Tests
docs/protocol.md                           Anmeldung mit Server-Adresse, Pflichten des Servers
app/src-tauri/src/
  errors.rs (neu)     ErrorCode, CommandError { code, detail }
  settings.rs (neu)   AppSettings, load(), save()
  prompts.rs (neu)    PromptRegistry (Fingerprint-Fragen, abbrechbar)
  sessions.rs         SessionStatus::Ended { reason, code, detail }, EndReason::ConnectFailed,
                      KeyGate bricht Hello ab, meldet "hello-unavailable", touch_recent
  hello.rs            Abbruch des Hello-Dialogs beim Verwerfen der Future
  passcli.rs          PassCliStatus::Failed, Zeitlimit, nur absolute PATH-Einträge
  commands/*.rs       ErrorCode statt Strings; settings, recent, system_locale
  lib.rs              single-instance
app/src/
  i18n/      en.json, de.json, i18n.tsx, language.ts, errors.ts, i18n.test.ts,
             no-hardcoded-text.test.ts
  settings/  settings.ts, SettingsProvider.tsx, themes.ts, themes.test.ts
  shell/     AppShell.tsx, TabStrip.tsx, NavSidebar.tsx, Drawer.tsx, ContextMenu.tsx,
             ConfirmDialog.tsx, ErrorMessage.tsx, shortcuts.ts, shortcuts.test.ts, AppShell.test.tsx
  pages/     HostsPage.tsx, hostsView.ts, hostsView.test.ts, KeysPage.tsx, SettingsPage.tsx,
             SettingsPage.test.tsx, PlaceholderPage.tsx, NewTabPage.tsx
  palette/   CommandPalette.tsx, rank.ts, rank.test.ts
  assets/    logo.svg
  state/tabs.ts (+ test)   Tab-Arten home, new, session
  components/              HostForm, GroupForm, KeyManager, ProtonPassImport, SettingsFields,
                           SshConfigImport, HostKeyDialog, TerminalView bleiben, übersetzt;
                           SessionStatusLine.tsx (aus TerminalTabs.tsx); Sidebar.tsx und
                           TerminalTabs.tsx entfallen
  App.tsx                  nur noch Provider und AppShell
```

---

### Task 1: Übersetzungsschicht

**Files:**
- Create: `app/src/i18n/en.json`, `de.json`, `i18n.tsx`, `language.ts`, `errors.ts`, `i18n.test.ts`, `no-hardcoded-text.test.ts`

**Interfaces:**
- Produces:
  - `type Language = "en" | "de"`, `type LanguageSetting = "auto" | Language`
  - `resolveLanguage(setting: LanguageSetting, systemLocale: string | null): Language`
  - `type Messages = Record<string, string>` (flache Schlüssel wie `hosts.title`)
  - `translate(lang: Language, key: string, params?: Record<string, string | number>): string`
  - `<I18nProvider language={Language}>`, `useT(): (key, params?) => string`, `useLanguage(): Language`
  - `type CommandError = { code: string; detail: string | null }`
  - `errorText(t, e: unknown): string`, `errorDetail(e: unknown): string | null`

- [ ] **Step 1: Tests schreiben** (`i18n.test.ts`)

```ts
const t = (lang: Language) => (key: string, params?: Record<string, string | number>) => translate(lang, key, params);
test("en and de have the same keys", () => expect(Object.keys(de).sort()).toEqual(Object.keys(en).sort()));
test("placeholders match per key", () => {
  for (const k of Object.keys(en)) expect(placeholders(de[k])).toEqual(placeholders(en[k])); // {name} sets
});
test("no empty values and no dashes", () => {
  for (const v of [...Object.values(en), ...Object.values(de)]) { expect(v.trim()).not.toBe(""); expect(v).not.toMatch(/[—–]/); }
});
test.each([
  ["auto", "de-DE", "de"], ["auto", "de_AT.UTF-8", "de"], ["auto", "DE", "de"], ["auto", "fr-FR", "en"],
  ["auto", "C", "en"], ["auto", "", "en"], ["auto", null, "en"], ["en", "de-DE", "en"], ["de", "en-US", "de"],
])("resolveLanguage(%s, %s) is %s", (s, l, want) => expect(resolveLanguage(s as LanguageSetting, l)).toBe(want));
test("params are filled in", () => expect(translate("en", "hosts.deleteConfirm", { name: "web" })).toContain("web"));
test("plural", () => {
  expect(translate("en", "hosts.count", { count: 1 })).toBe("1 host");
  expect(translate("en", "hosts.count", { count: 2 })).toBe("2 hosts");
  expect(translate("de", "hosts.count", { count: 1 })).toBe("1 Host");
});
test("missing de key falls back to en", () => expect(translateWith({ a: "A" }, {}, "de", "a")).toBe("A"));
test("known code is translated", () => expect(errorText(t("en"), { code: "agent_unavailable", detail: null })).toBe(en["errors.agent_unavailable"]));
test("unknown code and plain strings use the generic text", () => {
  expect(errorText(t("en"), { code: "future_code", detail: "x" })).toBe(en["errors.unknown"]);
  expect(errorText(t("en"), "boom")).toBe(en["errors.unknown"]);
  expect(errorDetail("boom")).toBe("boom");
});
```

`translateWith(primary, fallback, lang, key, params?)` ist die reine Funktion hinter `translate`; Plural: ist `params.count` eine Zahl, wird `key + "." + new Intl.PluralRules(lang).select(count)` gesucht, dann `key + ".other"`. Fehlt ein Schlüssel auch im Englischen, kommt der Schlüssel selbst zurück.

- [ ] **Step 2: Test laufen lassen, er schlägt fehl** (`npm test -- i18n`, Module fehlen)
- [ ] **Step 3: Umsetzen.** Die JSON-Dateien enthalten zunächst die Schlüssel, die die Tests brauchen, dazu `errors.unknown`; jede spätere Aufgabe ergänzt ihre Schlüssel in beiden Dateien. `resolveLanguage`: die ersten zwei Buchstaben, klein geschrieben, `de` ergibt Deutsch.
- [ ] **Step 4: Wächter gegen festgeschriebene Texte** (`no-hardcoded-text.test.ts`): liest alle `src/**/*.tsx` außer Tests und meldet (a) JSX-Text zwischen `>` und `<` ohne `{`, der zwei aufeinanderfolgende Buchstaben enthält (`[A-Za-zÄÖÜäöüß]{2,}`), (b) Attribute `title`, `placeholder`, `aria-label`, `alt`, `label` mit festem Text, (c) jeden Aufruf von `alert(` oder `confirm(`. Erlaubt ist das Wort `SSHCraft`. Eine Konstante `PENDING` listet die Dateien, die noch nicht umgestellt sind (alle vorhandenen Komponenten und `App.tsx`); ein zweiter Test prüft, dass jede Datei in `PENDING` noch existiert, damit die Liste nicht veraltet. Gegenprobe: `PENDING` leeren, der Test muss die vorhandenen deutschen Texte melden.
- [ ] **Step 5: Tests grün** (`npm test`, `npm run typecheck`)
- [ ] **Step 6: Commit** `git commit -m "Übersetzungsschicht für Englisch und Deutsch mit AUTO"`

---

### Task 2: Fehlercodes aus Rust, Verbindung fehlgeschlagen, Tresor-Hinweis, eine Adressauflösung

**Files:**
- Create: `app/src-tauri/src/errors.rs`
- Modify: `app/src-tauri/src/commands/{mod,data,keys,sessions,sshconfig}.rs`, `sessions.rs`, `sessions_tests.rs`, `app/src-tauri/Cargo.toml`, `crates/sshcraft-core/src/model.rs`, `app/src/api.ts`, `app/src/i18n/*.json`

**Interfaces:**
- Consumes: `en.json`, `de.json` aus Task 1.
- Produces:
  - `#[derive(Serialize, strum::EnumIter, strum::IntoStaticStr, Clone, Copy, Debug, PartialEq, Eq)] #[serde(rename_all = "snake_case")] #[strum(serialize_all = "snake_case")] pub enum ErrorCode` mit genau: `Store, Io, Internal, NotFound, MasterKeyMissing, InvalidKey, WrongPassphrase, Vault, VaultLocked, PassCliMissing, PassCliLoggedOut, PassCliFailed, PassCliTimeout, NoPrivateKey, Encrypted, AgentUnavailable, AgentKeyMissing, AgentRefused, HelloDeclined, HostWrongGroup, HostMissingAddress, HostMissingUsername, HostMissingKey, Network, Session`
  - `pub struct CommandError { pub code: ErrorCode, pub detail: Option<String> }`, `CommandError::new(code, detail: impl ToString)`, `CommandError::code(code)` (ohne detail); serde-Form `{ "code": "...", "detail": ... }`
  - `impl From<ResolveError> for ErrorCode` (`WrongGroup` zu `HostWrongGroup` usw.)
  - `SessionStatus::Ended { reason: EndReason, code: Option<ErrorCode>, detail: Option<String> }`
  - `EndReason::ConnectFailed` (neu, serde `connect_failed`)
  - `sshcraft_core::model::effective_endpoint(host: &Host, group: Option<&Group>) -> Result<(String, u16), ResolveError>`; `resolve()` und `forget_host_key` benutzen sie
  - `vault_status` liefert `{ ok: bool, code: ErrorCode | null, detail: string | null }`
  - TS: `CommandError = { code: string; detail: string | null }` in `api.ts`, `errorMessage` entfällt

Zuordnung der bisherigen Stellen: `"store"` zu `Store`, `"io"` zu `Io`, `"internal"` zu `Internal`, `"not_found"` zu `NotFound`, `"incomplete"` zu dem `ResolveError` aus `effective_endpoint`, `"vault_locked"` zu `VaultLocked`, `"agent_unavailable"` zu `AgentUnavailable`, `"session"` zu `Session`, `"pass_cli"` zu `PassCliFailed`. In `sessions.rs` entfallen `AGENT_UNAVAILABLE`, `AGENT_KEY_MISSING`, `AGENT_REFUSED` und "Windows Hello wurde abgebrochen." zugunsten der Codes; `KeyGate::ensure` gibt `Result<(), ErrorCode>` zurück; `lookup` gibt `Result<ResolvedHost, (ErrorCode, Option<String>)>` zurück. `Failure::Lost` trägt `Network` plus den Text des Fehlers als `detail`.

- [ ] **Step 1: Tests schreiben**
  - `errors.rs`: `every_error_code_has_a_translation`: liest `include_str!("../../src/i18n/en.json")` und `de.json`, prüft für jedes `ErrorCode::iter()` den Schlüssel `errors.<code>`. `command_error_serializes_as_code_and_detail`: `serde_json::to_value(CommandError::new(ErrorCode::AgentRefused, "x"))` ist `{"code":"agent_refused","detail":"x"}`.
  - `sessions_tests.rs`: `first_failed_attempt_is_connect_failed` (Ziel-Port ohne Server: `Ended { reason: ConnectFailed, code: Some(Network), .. }`, keine `Reconnecting`-Meldung); `lost_after_connect_still_reconnects` (bestehender Test bleibt grün); vorhandene Tests auf die neuen Codes umstellen (`AgentUnavailable`, `AgentKeyMissing`, `AgentRefused`, `HelloDeclined`, `VaultLocked`).
  - `model.rs`: `effective_endpoint_inherits_address_and_port` und `effective_endpoint_needs_no_username_or_key` (Host ohne Benutzer und Schlüssel liefert trotzdem Adresse und Port 22).
- [ ] **Step 2: Tests laufen lassen, sie schlagen fehl** (`cargo test -p sshcraft-app -p sshcraft-core`)
- [ ] **Step 3: Umsetzen.** `strum` 0.28.0 mit Feature `derive`. In `run` wird ein `Failure::Lost` vor der ersten erfolgreichen Verbindung zu `EndReason::ConnectFailed` statt `Lost`.
- [ ] **Step 4: Gegenprobe:** einen Schlüssel aus `de.json` entfernen, `every_error_code_has_a_translation` muss rot werden; Schlüssel zurück.
- [ ] **Step 5: Oberfläche nachziehen.** `api.ts`: `SessionStatus` und `EndReason` um `code` und `connect_failed` erweitern. Übersetzungsschlüssel `errors.<code>` für alle Codes (deutsche Texte aus den bisherigen Meldungen übernehmen, z. B. `errors.agent_unavailable` = "Proton Pass ist nicht erreichbar. Läuft die Proton-Pass-App?"), `session.ended.<reason>` für jeden `EndReason` (`session.ended.connect_failed` = "Verbindung fehlgeschlagen" / "Connection failed"), `banner.vault.master_key_missing` (der bisherige lange Hinweis) und `banner.vault.other` ("Der Schlüsseltresor lässt sich nicht öffnen: {reason}"). Das Banner wählt den Text nach `code`. Die Komponenten benutzen vorerst `errorText(t, e)`; ihre übrigen Texte zieht Task 9 um.
- [ ] **Step 6: Tests grün** (`cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `npm test`, `npm run typecheck`)
- [ ] **Step 7: Commit** `git commit -m "Fehlercodes statt Texten aus Rust, Verbindung fehlgeschlagen beim ersten Versuch"`

---

### Task 3: `pass-cli` und ssh-config-Import nachschärfen

**Files:**
- Modify: `app/src-tauri/src/passcli.rs`, `passcli_tests.rs`, `commands/keys.rs`, `commands/sshconfig.rs`, `crates/sshcraft-core/src/sshconfig.rs`, `sshconfig_tests.rs`, `app/src/components/SshConfigImport.tsx`, `ProtonPassImport.tsx`, `app/src/api.ts`, `app/src/i18n/*.json`, `app/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: `ErrorCode` aus Task 2.
- Produces:
  - `PassCliStatus = NotInstalled | LoggedOut | Ready | Failed` (serde `failed`); `PassCliError::Timeout` zu `ErrorCode::PassCliTimeout`
  - `pub fn classify_failure(code: Option<i32>, stderr: &[u8]) -> PassCliError`: `LoggedOut` nur, wenn die erste Zeile von stderr (ohne Groß- und Kleinschreibung) `not logged in`, `logged out`, `login required` oder `pass-cli login` enthält; sonst `Failed`
  - `pub fn run_with_timeout(command: Command, limit: Duration) -> Result<Output, PassCliError>`: beendet den Prozess nach `limit` und gibt `Timeout` zurück. Limits: `info` und `item list` 15 s, `item view` 120 s (Proton Pass fragt eventuell nach)
  - `pub fn find_on_path(path_var: &OsStr, exe_name: &str) -> Option<PathBuf>`: nur absolute Einträge
  - `sshcraft_core::sshconfig::KeyNote = NoIdentityFile | NotInAgent | AgentUnavailable` (serde snake_case), `ImportCandidate.key_note: Option<KeyNote>`
  - `ImportWarning = ProxyIgnored { directive: String } | UnsupportedToken { token: String }`, `ImportCandidate.warnings: Vec<ImportWarning>`
  - `plan_import(config, existing, agent_fingerprints: Option<&[String]>)`: `None` heißt "Agent nicht erreichbar"

- [ ] **Step 1: Tests schreiben**
  - `classify_failure_only_reports_logged_out_for_login_messages`: `"Error: not logged in"` ergibt `LoggedOut`, `"error: network unreachable"` ergibt `Failed`, leeres stderr ergibt `Failed`. Gegenprobe: Muster immer auf `LoggedOut` stellen, Test rot.
  - `run_with_timeout_kills_slow_process` (unter Windows `powershell -NoProfile -Command Start-Sleep 10`, sonst `sleep 10`): Limit 300 ms, Ergebnis `Err(Timeout)` nach höchstens 3 s.
  - `find_on_path_skips_relative_entries`: PATH aus einem relativen Ordner mit `pass-cli.exe` (in einem Temp-Ordner als aktuellem Verzeichnis) und einem absoluten Ordner ohne; Ergebnis `None`. Gegenprobe: Prüfung `is_absolute` entfernen, Test rot.
  - `sshconfig_tests.rs`: `preview_without_agent_says_agent_unavailable` (`None` ergibt `KeyNote::AgentUnavailable` für Hosts mit `IdentityFile`, `NoIdentityFile` bleibt); `proxy_jump_and_proxy_command_are_warned` (beide Direktiven erscheinen als `ProxyIgnored`); `percent_h_in_hostname_is_the_alias` (`HostName %h.example.org` bei Alias `web` ergibt `web.example.org`, `%%` ergibt `%`); `other_tokens_are_warned` (`HostName %r.x` ergibt `UnsupportedToken { token: "%r" }`, Adresse bleibt unverändert).
- [ ] **Step 2: Tests laufen lassen, sie schlagen fehl**
- [ ] **Step 3: Umsetzen.** `PassCliProcess::status` bildet `Ok` auf `Ready`, `LoggedOut` auf `LoggedOut`, alles andere auf `Failed` ab. Der Befehl `ssh_config_preview` übergibt `None`, wenn `list_agent_keys` scheitert. `wait-timeout` in der neuesten Version; stdout und stderr werden in eigenen Threads gelesen, damit ein voller Puffer das Warten nicht blockiert; stdout bleibt in `Zeroizing`.
- [ ] **Step 4: Oberfläche.** `SshConfigImport`: Hinweistexte aus `sshconfig.keyNote.<note>` und `sshconfig.warning.proxyIgnored` / `unsupportedToken`; ein Kandidat mit Warnung ist in der Vorschau anfangs nicht angehakt; die Liste der übersprungenen Zeilen benutzt den Index als React-Schlüssel (`key={index}`), weil Zeilen doppelt vorkommen dürfen. `ProtonPassImport` zeigt bei `failed` den Text `keys.passCli.failed`.
- [ ] **Step 5: Tests grün** (`cargo test -p sshcraft-app -p sshcraft-core`, `npm test`)
- [ ] **Step 6: Commit** `git commit -m "pass-cli mit Zeitlimit und echten Fehlern, ssh-config warnt vor Proxy und Platzhaltern"`

---

### Task 4: Fingerprint-Frage, Windows Hello und Hello-Hinweis beim Schließen eines Tabs

**Files:**
- Create: `app/src-tauri/src/prompts.rs`
- Modify: `app/src-tauri/src/state.rs`, `sessions.rs`, `sessions_tests.rs`, `hello.rs`, `commands/sessions.rs`, `app/src/App.tsx` (bis Task 7 der Ort der Ereignisse), `app/src/api.ts`

**Interfaces:**
- Produces:
  - `pub trait PromptEmitter: Send + Sync + 'static { fn asked(&self, prompt: &HostKeyPrompt) -> bool; fn withdrawn(&self, prompt_id: u64); }`
  - `pub struct PromptRegistry<E: PromptEmitter>` mit `async fn ask(&self, host_name, address, port, fingerprint) -> bool` und `fn answer(&self, prompt_id, accept)`. Wird die Future von `ask` verworfen (Sitzung abgebrochen), entfernt ein Drop-Wächter den Eintrag und ruft `withdrawn(prompt_id)`. `TauriPrompter` wird zu `PromptRegistry<TauriEmitter>`; `withdrawn` sendet das Ereignis `host-key-prompt-withdrawn` mit `{ prompt_id }`.
  - `UserPresence::verify` muss abbrechbar sein: `WindowsHello` hält die `IAsyncOperation` in einem Wächter, der beim Verwerfen `Cancel()` aufruft.
  - `KeyGate::new(presence, on_unavailable: Box<dyn Fn() + Send + Sync>)`; beim ersten `Presence::Unavailable` wird `on_unavailable` genau einmal gerufen; die App sendet damit das Ereignis `hello-unavailable`.
  - `session_close` bricht die Aufgabe der Sitzung mit `JoinHandle::abort` ab (falls sie das noch nicht tut).
- Consumes: `SessionStatus` aus Task 2.

- [ ] **Step 1: Tests schreiben**
  - `prompts.rs`: `dropped_question_is_withdrawn` (Fake-Emitter; `ask` in einer Aufgabe starten, Aufgabe abbrechen; `withdrawn` wurde mit derselben ID gerufen, `answer` für diese ID ist danach wirkungslos); `answered_question_is_not_withdrawn`.
  - `sessions_tests.rs`: `closing_tab_cancels_hello` (Fake-`UserPresence`, der bis zum Abbruch wartet und den Abbruch zählt; Sitzung öffnen, schließen; Zähler 1); `waiting_tab_asks_again_after_other_tab_closed` (zwei Sitzungen, die erste wird geschlossen, die zweite bekommt eine neue Abfrage und verbindet nach `Verified`); `hello_unavailable_is_reported_once` (zwei Sitzungen mit `Unavailable`, Rückruf genau einmal). Gegenprobe für den ersten Test: Wächter auskommentieren, Test rot.
- [ ] **Step 2: Tests laufen lassen, sie schlagen fehl**
- [ ] **Step 3: Umsetzen** (siehe Interfaces).
- [ ] **Step 4: Oberfläche.** `App.tsx` hört auf `host-key-prompt-withdrawn` und entfernt die Frage aus der Warteschlange; das Banner zu Windows Hello hängt an `hello-unavailable` statt an der Abfrage beim ersten verbundenen Tab; `hello_status` bleibt nur für den Start der Oberfläche.
- [ ] **Step 5: Tests grün** (`cargo test -p sshcraft-app`, `cargo clippy -p sshcraft-app -- -D warnings`, `npm test`)
- [ ] **Step 6: Commit** `git commit -m "Fingerprint-Frage und Windows Hello enden mit ihrem Tab, Hello-Hinweis über ein Ereignis"`

---

### Task 5: Einstellungen und letzte Verbindungen speichern

**Files:**
- Create: `app/src-tauri/src/settings.rs`
- Modify: `crates/sshcraft-core/src/store.rs`, `store_tests.rs`, `app/src-tauri/src/sessions.rs`, `sessions_tests.rs`, `commands/data.rs`, `lib.rs`, `Cargo.toml`, `app/src/api.ts`

**Interfaces:**
- Produces:
  - Store, Schema-Version 2 (Migration `if version < 2`, ohne die Tabellen von Version 1 anzufassen):
    `CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);`
    `CREATE TABLE recent_hosts (host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE, connected_at INTEGER NOT NULL);`
  - `Store::get_setting(&self, key: &str) -> Result<Option<String>, StoreError>`, `put_setting(&self, key: &str, value: &str)`, `touch_recent(&self, host: HostId, at_ms: i64)`, `recent_hosts(&self, limit: u32) -> Result<Vec<HostId>, StoreError>` (neueste zuerst)
  - `settings.rs`: `AppSettings { language: LanguageSetting, app_theme: AppTheme, terminal_theme: String, font_family: String, font_size: u16, host_view: HostView }` mit `LanguageSetting = Auto | En | De`, `AppTheme = System | Dark | Light`, `HostView = Grid | List` (alle serde snake_case). Vorgaben: `auto`, `system`, `"sshcraft-dark"`, `"Cascadia Mono, Consolas, monospace"`, `14`, `grid`. Gespeichert als JSON unter dem Schlüssel `app`.
  - `pub fn load(store: &Store) -> AppSettings`: liest das JSON als `serde_json::Value` und übernimmt jedes Feld einzeln; fehlt es oder passt es nicht, gilt die Vorgabe. Kein JSON: alle Vorgaben.
  - `pub fn save(store: &Store, settings: AppSettings) -> Result<AppSettings, StoreError>`: `font_size` auf 8 bis 32 begrenzt, `font_family` getrimmt, leer oder länger als 200 Zeichen ergibt die Vorgabe; gibt den gespeicherten Stand zurück.
  - Befehle: `get_settings() -> AppSettings`, `save_settings(settings) -> AppSettings`, `system_locale() -> Option<String>` (`sys_locale::get_locale()`), `list_recent(limit: u32) -> Vec<String>`
  - Sitzungen rufen `touch_recent(host_id, jetzt in ms)` bei jedem `Connected`.
  - TS: `AppSettings` mit denselben Feldnamen, `getSettings`, `saveSettings`, `systemLocale`, `listRecent` in `api.ts`

- [ ] **Step 1: Tests schreiben**
  - `store_tests.rs`: `upgrade_from_v1_keeps_data` (Datei mit dem Schema von Version 1, `user_version = 1`, einer Gruppe, einem Host, einem Schlüssel, einem Fingerprint anlegen; `Store::open`; alles noch da, `user_version` ist 2, `put_setting` geht); `recent_hosts_newest_first_and_limited`; `deleting_host_removes_recent`; `touch_recent_twice_keeps_one_row`. Gegenprobe für den ersten Test: Migration so ändern, dass sie `SCHEMA_V1` erneut ausführt, Test rot. `SCHEMA_V1` wird dafür `pub(crate)`.
  - `settings.rs`: `missing_fields_take_defaults` (`{"language":"de"}`), `unknown_value_only_resets_that_field` (`{"language":"fr","font_size":16}` ergibt `Auto` und 16), `not_json_gives_defaults`, `font_size_is_clamped` (0 ergibt 8, 500 ergibt 32), `blank_font_family_gives_default`. Gegenprobe für den zweiten Test: ganzes Objekt mit `serde_json::from_str::<AppSettings>` lesen, Test rot.
  - `sessions_tests.rs`: `connected_session_is_recent`.
- [ ] **Step 2: Tests laufen lassen, sie schlagen fehl**
- [ ] **Step 3: Umsetzen.** `sys-locale` 0.3.2.
- [ ] **Step 4: Tests grün** (`cargo test --workspace`, `cargo clippy --workspace -- -D warnings`)
- [ ] **Step 5: Commit** `git commit -m "Einstellungen und letzte Verbindungen im Speicher, Schema 2"`

---

### Task 6: Themes, Schriften und Einstellungs-Kontext

**Files:**
- Create: `app/src/settings/settings.ts`, `SettingsProvider.tsx`, `themes.ts`, `themes.test.ts`
- Modify: `app/src/styles.css`, `app/src/components/TerminalView.tsx`, `app/src/App.tsx`

**Interfaces:**
- Consumes: `getSettings`, `saveSettings`, `systemLocale` aus Task 5; `I18nProvider`, `resolveLanguage` aus Task 1.
- Produces:
  - `themes.ts`: `type TerminalTheme = { id: string; name: string; colors: ITheme }` (xterm `ITheme` mit `background`, `foreground`, `cursor`, `selectionBackground` und den 16 ANSI-Farben); `TERMINAL_THEMES` mit den IDs `sshcraft-dark`, `sshcraft-light`, `solarized-dark`, `solarized-light`, `nord`, `dracula` (Farbwerte der öffentlichen, MIT-lizenzierten Schemata); `terminalTheme(id: string): TerminalTheme` (unbekannt ergibt `sshcraft-dark`); `resolveAppTheme(setting: AppTheme, prefersDark: boolean): "dark" | "light"`; `FONT_PRESETS = ["Cascadia Mono", "Consolas", "JetBrains Mono", "Fira Code", "Courier New"]`
  - `<SettingsProvider>`: lädt `getSettings()` und `systemLocale()`, rendert bis dahin nichts, setzt `data-theme` auf `<html>` (folgt bei `system` dem `matchMedia("(prefers-color-scheme: dark)")`), umschließt die Kinder mit `I18nProvider`. `useSettings(): { settings: AppSettings; update(patch: Partial<AppSettings>): Promise<void> }`; `update` speichert und übernimmt die Antwort aus Rust (begrenzte Werte).
  - `styles.css`: Farben nur als Variablen unter `:root[data-theme="dark"]` und `:root[data-theme="light"]`: `--bg`, `--bg-panel`, `--bg-raised`, `--bg-hover`, `--border`, `--text`, `--text-muted`, `--accent: #f5a524`, `--accent-text`, `--danger`, `--warning`, `--radius: 8px`. Dunkel: `--bg #111318`, `--bg-panel #171a21`, `--bg-raised #1f232c`, `--text #e7e8ec`, `--text-muted #8b90a0`. Hell: `--bg #f6f6f8`, `--bg-panel #ffffff`, `--bg-raised #eceef2`, `--text #1b1d23`, `--text-muted #5f6472`.
  - `TerminalView` übernimmt `fontFamily`, `fontSize` und das Theme aus `useSettings()` und setzt sie bei jeder Änderung über `terminal.options`, danach `fit()`.

- [ ] **Step 1: Tests schreiben** (`themes.test.ts`): jedes Theme hat alle 20 Farbfelder als `#rrggbb`; IDs eindeutig; `terminalTheme("missing").id === "sshcraft-dark"`; `resolveAppTheme("system", true) === "dark"`, `("system", false) === "light"`, `("light", true) === "light"`.
- [ ] **Step 2: Test laufen lassen, er schlägt fehl**
- [ ] **Step 3: Umsetzen** (siehe Interfaces); `App.tsx` umschließt den bisherigen Inhalt mit `SettingsProvider`.
- [ ] **Step 4: Tests grün** (`npm test`, `npm run typecheck`)
- [ ] **Step 5: Commit** `git commit -m "Themes für App und Terminal, Schriften aus den Einstellungen"`

---

### Task 7: Neuer Rahmen mit Tab-Leiste, Startseite, Navigation und Tastenkürzeln

**Files:**
- Create: `app/src/shell/AppShell.tsx`, `TabStrip.tsx`, `NavSidebar.tsx`, `Drawer.tsx`, `ContextMenu.tsx`, `ConfirmDialog.tsx`, `ErrorMessage.tsx`, `shortcuts.ts`, `shortcuts.test.ts`, `AppShell.test.tsx`, `app/src/components/SessionStatusLine.tsx`, `app/src/pages/PlaceholderPage.tsx`, `app/src/assets/logo.svg`
- Modify: `app/src/state/tabs.ts`, `tabs.test.ts`, `app/src/App.tsx`, `app/src/components/TerminalView.tsx`, `app/package.json`
- Delete: `app/src/components/Sidebar.tsx`, `TerminalTabs.tsx`, `TerminalTabs.test.tsx` (der Test für `EndedLine` zieht nach `SessionStatusLine.test.tsx` um)

**Interfaces:**
- Consumes: `useT`, `errorText`, `errorDetail` (Task 1), `useSettings` (Task 6), Ereignisse aus Task 4.
- Produces:
  - `tabs.ts`: `type Tab = { kind: "home"; id: "home" } | { kind: "new"; id: string } | { kind: "session"; id: string; hostId: string; title: string; sessionId?: string; status: SessionStatus }`; `initialTabs = { tabs: [home], activeId: "home" }`; Aktionen `open` (Sitzungstab), `openNew { id }`, `pick { id, hostId, title }` (macht aus einem `new`-Tab an derselben Stelle einen Sitzungstab), `attach`, `status`, `close`, `activate`, `next`, `prev`
  - `shortcuts.ts`: `type ShortcutAction = "palette" | "newTab" | "closeTab" | "nextTab" | "prevTab"`; `matchShortcut(e: { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }, context: "terminal" | "app"): ShortcutAction | null`. Belegung: Strg+Umschalt+P Palette (überall), Strg+K Palette (nur `app`), Strg+Umschalt+T neuer Tab, Strg+Umschalt+W Tab schließen, Strg+Tab und Strg+Bild-ab nächster Tab, Strg+Umschalt+Tab und Strg+Bild-auf voriger Tab. Verglichen wird `e.key.toLowerCase()`.
  - `TerminalView` hängt `terminal.attachCustomKeyEventHandler((e) => matchShortcut(e, "terminal") === null)` ein; `AppShell` hört auf `keydown` am `window` und führt die Aktion aus.
  - `<AppShell>`: oben `TabStrip` (Startseiten-Tab mit Logo, Sitzungstabs mit Statuspunkt, `+` für einen neuen Tab), darunter bei der Startseite `NavSidebar` (Hosts, Schlüssel, Snippets, Portweiterleitung, Protokolle; unten Einstellungen) plus Seite, bei einem Sitzungstab das Terminal über die volle Breite mit `SessionStatusLine`. Alle Sitzungstabs bleiben eingehängt und werden mit `visibility: hidden` verborgen. `type Page = "hosts" | "keys" | "snippets" | "forwarding" | "logs" | "settings"`.
  - `<Drawer open title onClose>`: Seitenpanel rechts, 440 px, Escape schließt. `<ContextMenu items={{ label, onSelect, danger? }[]} at={{ x, y }} onClose>`. `<ConfirmDialog message confirmLabel onConfirm onCancel>` ersetzt `confirm()`. `<ErrorMessage error>` zeigt `errorText` und aufklappbar `errorDetail`.
  - `PlaceholderPage({ page })` für Snippets, Portweiterleitung, Protokolle mit `placeholder.<page>.title` und `placeholder.text` ("Kommt in einer späteren Version." / "Coming in a later version.").
  - `logo.svg`: eigenes Zeichen, abgerundetes Quadrat in `#1f232c`, darin ein Chevron `>` und ein kurzer Unterstrich als Cursor in `#f5a524`.

- [ ] **Step 1: Tests schreiben**
  - `shortcuts.test.ts`: im Kontext `terminal` ergeben Strg+W, Strg+K, Strg+R, Strg+T, Strg+C, Strg+D, Strg+L, Strg+Umschalt+C, Strg+Umschalt+V, Alt+B und Alt+F jeweils `null`; Strg+Umschalt+T ergibt `newTab` (auch mit `key: "T"`), Strg+Umschalt+W `closeTab`, Strg+Umschalt+P `palette`, Strg+Tab `nextTab`, Strg+Umschalt+Tab `prevTab`; Strg+K ergibt im Kontext `app` `palette`. Gegenprobe: Strg+K auch im Terminal zulassen, Test rot.
  - `tabs.test.ts`: Startseite lässt sich nicht schließen; `openNew` hängt an und aktiviert; `pick` behält Position und ID und setzt `connecting`; Schließen des letzten Sitzungstabs aktiviert die Startseite; `next` und `prev` laufen im Kreis über alle Tabs samt Startseite.
  - `AppShell.test.tsx` (mit `vi.mock` für `@tauri-apps/api/core`, `@tauri-apps/api/event` und `TerminalView`, das Ein- und Aushängen zählt): Sitzungstab öffnen, Startseite aktivieren, zurück zum Tab; `TerminalView` wurde einmal eingehängt und nie ausgehängt. Zweiter Test: Ereignis `host-key-prompt-withdrawn` entfernt den offenen Dialog.
- [ ] **Step 2: Tests laufen lassen, sie schlagen fehl**
- [ ] **Step 3: Umsetzen.** `lucide-react` 1.72.0 installieren (`npm i lucide-react@1.72.0`). `App.tsx` enthält danach nur `SettingsProvider` und `AppShell`; Banner (Tresor, Hello, Ladefehler) stehen in `AppShell` über dem Inhalt aller Tabs. Die Seite `hosts` zeigt bis Task 8 die bisherigen Formulare im `Drawer`. Den Aufbau von Termius (auf dem PC installiert) darf man sich für Abstände und Abläufe ansehen; übernommen werden keine Grafiken, Farben oder Texte.
- [ ] **Step 4: `PENDING` im Wächter aus Task 1** um die neuen und gelöschten Dateien bereinigen; alle neuen Dateien sind frei von festen Texten.
- [ ] **Step 5: Tests grün** (`npm test`, `npm run typecheck`, `npm run build`)
- [ ] **Step 6: Commit** `git commit -m "Neuer Rahmen mit Tab-Leiste, Startseite, Navigation und Tastenkürzeln"`

---

### Task 8: Hosts-Seite

**Files:**
- Create: `app/src/pages/HostsPage.tsx`, `hostsView.ts`, `hostsView.test.ts`
- Modify: `app/src/shell/AppShell.tsx`, `app/src/components/HostForm.tsx`, `GroupForm.tsx`, `SettingsFields.tsx`, `SshConfigImport.tsx`, `app/src/i18n/*.json`, `app/src/styles.css`

**Interfaces:**
- Consumes: `Drawer`, `ContextMenu`, `ConfirmDialog` (Task 7), `useSettings` für `host_view` (Task 6).
- Produces:
  - `normalize(s: string): string` (`s.normalize("NFC").toLocaleLowerCase()`)
  - `filterHosts(hosts: Host[], groups: Group[], query: string): Host[]`: Treffer in Name, Adresse, Benutzer (auch geerbt) und Gruppenname; leere Suche gibt alle zurück
  - `hostSubtitle(host: Host, group: Group | undefined): string | null`: `benutzer@adresse`, Port nur wenn nicht 22 (`root@10.0.0.1:2222`); ohne Adresse `null`
  - `sortByName<T extends { name: string }>(items: T[], lang: Language): T[]` (`localeCompare` mit `sensitivity: "base"`)
  - `<HostsPage>`: Werkzeugleiste mit Suchfeld (Platzhalter `hosts.search`), Knopf "Neu" mit Menü (Host, Gruppe, Aus ssh-config importieren), Umschalter Kacheln/Liste (speichert `host_view`). Ohne Suche: Abschnitt Gruppen (Kachel mit Name und Zahl der Hosts, `hosts.count`), darunter Hosts ohne Gruppe und je Gruppe ihre Hosts. Mit Suche: eine flache Trefferliste. Kachel: Icon `Server`, Name (mit Auslassungspunkten gekürzt), `hostSubtitle` oder `hosts.noAddress`. Doppelklick verbindet, Rechtsklick öffnet das Kontextmenü (Verbinden, Bearbeiten, Löschen), Löschen fragt über `ConfirmDialog`. Bearbeiten und Anlegen öffnen den `Drawer`.

- [ ] **Step 1: Tests schreiben** (`hostsView.test.ts`): "MÜNCHEN" findet Host "münchen"; ein Host mit dem Namen `"Mu\u0308nchen"` (NFD) wird mit "münchen" gefunden; Emoji-Name "🚀 prod" wird mit "prod" gefunden; Treffer über geerbten Benutzer der Gruppe; Treffer über Gruppennamen; `hostSubtitle` mit geerbtem Benutzer und Adresse, Port 22 ohne Angabe, Port 2222 mit Angabe, ohne Adresse `null`; `sortByName` sortiert "Äpfel" für `de` zwischen "Apfel" und "Birne".
- [ ] **Step 2: Test laufen lassen, er schlägt fehl**
- [ ] **Step 3: Umsetzen.** `HostForm`, `GroupForm`, `SettingsFields` und `SshConfigImport` bekommen alle Texte über `useT` und fallen aus `PENDING`.
- [ ] **Step 4: Tests grün** (`npm test`, `npm run typecheck`)
- [ ] **Step 5: Commit** `git commit -m "Hosts-Seite mit Kacheln, Suche und Seitenpanel"`

---

### Task 9: Schlüssel-Seite und Einstellungen-Seite

**Files:**
- Create: `app/src/pages/KeysPage.tsx`, `SettingsPage.tsx`, `SettingsPage.test.tsx`
- Modify: `app/src/components/KeyManager.tsx`, `ProtonPassImport.tsx`, `HostKeyDialog.tsx`, `HostKeyDialog.test.tsx`, `SessionStatusLine.tsx`, `app/src/shell/AppShell.tsx`, `app/src/i18n/*.json`

**Interfaces:**
- Consumes: `useSettings`, `TERMINAL_THEMES`, `FONT_PRESETS` (Task 6).
- Produces:
  - `<KeysPage>`: oben Knöpfe "Erzeugen", "Importieren", "Aus Proton Pass"; Abschnitt "In diesem Tresor" (Kacheln mit Name, Algorithmus, Fingerprint, Kontextmenü Kopieren des öffentlichen Schlüssels und Löschen); Abschnitt "Proton-Pass-Agent" (Schlüssel aus `listAgentKeys`, nur lesen; nicht erreichbar zeigt `errors.agent_unavailable`). Importieren und Proton Pass öffnen den `Drawer` mit dem bisherigen Inhalt aus `KeyManager` und `ProtonPassImport`.
  - `<SettingsPage>`: Abschnitt Allgemein (Sprache: AUTO, English, Deutsch; die Sprachnamen stehen in jeder Sprache gleich, `settings.language.auto` übersetzt), Abschnitt Aussehen (App-Theme: System, Dunkel, Hell; Terminal-Theme als Auswahl mit Farbvorschau aus `background` und vier ANSI-Farben; Schriftart als Eingabe mit `datalist` aus `FONT_PRESETS`; Schriftgröße 8 bis 32), Abschnitt Über (Version aus `@tauri-apps/api/app` `getVersion()`, Lizenz). Jede Änderung ruft sofort `update`.

- [ ] **Step 1: Tests schreiben** (`SettingsPage.test.tsx`, `invoke` gemockt, `get_settings` liefert `language: "de"`): die Überschrift lautet "Einstellungen"; Sprache auf "English" stellen ruft `save_settings` mit `language: "en"`, danach lautet die Überschrift "Settings"; Schriftgröße 500 eintragen, Rust antwortet mit 32, das Feld zeigt 32.
- [ ] **Step 2: Test laufen lassen, er schlägt fehl**
- [ ] **Step 3: Umsetzen.** `HostKeyDialog` und `SessionStatusLine` werden übersetzt (`session.status.*`, `session.ended.*`, `hostKey.*`); `SessionStatusLine` zeigt bei `ended` mit `code` den Text `errors.<code>` und darunter `detail` in Festbreitenschrift. Die Dateien fallen aus `PENDING`.
- [ ] **Step 4: Tests grün** (`npm test`, `npm run typecheck`)
- [ ] **Step 5: Commit** `git commit -m "Schlüssel-Seite und Einstellungen mit Sprache, Theme und Schrift"`

---

### Task 10: Neuer-Tab-Seite und Befehlspalette

**Files:**
- Create: `app/src/palette/CommandPalette.tsx`, `rank.ts`, `rank.test.ts`, `app/src/pages/NewTabPage.tsx`
- Modify: `app/src/shell/AppShell.tsx`, `app/src/i18n/*.json`

**Interfaces:**
- Consumes: `normalize`, `hostSubtitle` (Task 8), `listRecent` (Task 5), Tab-Aktionen und `ShortcutAction` (Task 7).
- Produces:
  - `type PaletteActionId = "newHost" | "newGroup" | "importSshConfig" | "openHosts" | "openKeys" | "openSettings" | "newTab" | "closeTab"`
  - `type PaletteItem = { kind: "host"; host: Host } | { kind: "action"; id: PaletteActionId; label: string }`
  - `rankPalette(query: string, hosts: Host[], groups: Group[], recent: string[], actions: { id: PaletteActionId; label: string }[]): PaletteItem[]`: leere Suche ergibt bis zu 5 letzte Hosts in der Reihenfolge von `recent` (unbekannte IDs übergehen), danach alle Aktionen. Sonst Hosts vor Aktionen; Hosts in Stufen: Name beginnt mit der Suche, ein Wort im Namen beginnt damit (Trenner Leerzeichen, `-`, `_`, `.`), Name enthält sie, Adresse oder Benutzer enthält sie; innerhalb einer Stufe zuletzt benutzte zuerst, dann nach Name. Höchstens 50 Einträge.
  - `<CommandPalette open onClose onPick>`: Eingabe oben, Liste darunter, Pfeil hoch und runter, Enter wählt, Escape schließt; ein Host öffnet einen Sitzungstab, eine Aktion wird ausgeführt.
  - `<NewTabPage tabId>`: Suchfeld mit Fokus, Abschnitt "Zuletzt verbunden" (aus `listRecent(10)`), darunter alle Hosts nach Gruppen; Enter verbindet den ersten Treffer; Auswahl ruft `pick` für diesen Tab.

- [ ] **Step 1: Tests schreiben** (`rank.test.ts`): leere Suche liefert Hosts aus `recent` in dieser Reihenfolge, höchstens 5, unbekannte IDs übergangen, danach Aktionen; Suche "web" ordnet `web-01` vor `my-web` vor `prodweb`; Suche "10.0.0" findet über die Adresse; eine Aktion mit passendem Namen steht hinter allen Host-Treffern; 80 passende Hosts ergeben 50 Einträge; "MÜNCHEN" findet "münchen".
- [ ] **Step 2: Test laufen lassen, er schlägt fehl**
- [ ] **Step 3: Umsetzen.** Nach jedem `connected` lädt `AppShell` die letzten Verbindungen neu.
- [ ] **Step 4: Tests grün** (`npm test`, `npm run typecheck`)
- [ ] **Step 5: Commit** `git commit -m "Befehlspalette und Neuer-Tab-Seite mit letzten Verbindungen"`

---

### Task 11: Restpunkte in `sshcraft-proto`

**Files:**
- Modify: `crates/sshcraft-proto/src/identity.rs`, `devicelog.rs`, `record.rs`, `crates/sshcraft-proto/tests/identity.rs`, `tests/devicelog.rs`, `docs/protocol.md`

**Interfaces:**
- Produces:
  - `DeviceKeys::sign_auth(&self, server_challenge: &[u8], device_id: Uuid, server_origin: &str) -> [u8; 64]` und `DeviceCertificate::verify_auth(&self, server_challenge: &[u8], server_origin: &str, signature: &[u8; 64]) -> Result<(), ProtoError>`. Signiert wird `signing_bytes(DOMAIN_AUTH, &[server_challenge, device_id, origin])` mit `origin = normalize_origin(server_origin)`.
  - `pub fn normalize_origin(url: &str) -> String`: Schema und Host klein, ohne Pfad, Abfrage und abschließenden Schrägstrich, Standardport (443 bei `https`, 80 bei `http`) weggelassen.

- [ ] **Step 1: Tests schreiben**
  - `identity.rs`: `auth_signature_is_bound_to_server_origin` (Signatur für `https://a.example` scheitert bei `https://b.example`); `origin_is_normalized` (`HTTPS://Sync.Example.org:443/path/` ergibt `https://sync.example.org`; `:8443` bleibt). Gegenprobe für den ersten Test: `origin` aus den signierten Teilen nehmen, Test rot.
  - `devicelog.rs`: `adding_same_device_twice_is_rejected`; `revoking_unknown_or_revoked_device_is_rejected` (beide Fälle); `verify_extends_with_empty_log_and_known_head_is_rejected`; `mutual_revocation_keeps_first_in_chain` (A sperrt B, danach kann B A nicht mehr sperren, weil B gesperrt ist; die Kette bleibt prüfbar). Wird ein Test rot, ist das ein Fehler im Crate und wird im selben Task behoben.
- [ ] **Step 2: Tests laufen lassen**; die neuen Anmelde-Tests schlagen fehl.
- [ ] **Step 3: Umsetzen.** `AccountKey::export` schreibt in einen vorab bemessenen `Zeroizing<String>` (`String::with_capacity(EXPORT_PREFIX.len() + BASE32_NOPAD.encode_len(body.len()))`, dann `encode_append`); `import` filtert in einen `Zeroizing<String>` mit `String::with_capacity(text.len())`; `DeviceKeys::generate` hält `private.to_bytes()` in `Zeroizing`. `verify_extends` benutzt `usize::try_from(known.seq)` und lehnt bei Überlauf mit dem vorhandenen Fehler für einen ungültigen Kopf ab. Diese Speicherpflege hat keinen eigenen Test; der Review prüft sie.
- [ ] **Step 4: Doku.** Rustdoc: `SealedRecord::open` setzt eine geprüfte Bereichsbeschreibung voraus; `verify_auth` verlangt, dass der Server zusätzlich prüft, ob das Gerät im Geräteprotokoll aktiv ist. `docs/protocol.md`: Anmeldung bindet die Server-Adresse (mit `normalize_origin`), Geräteschlüssel gelten je Server; ein Server muss `DeviceLog::verify` bei jedem Anhängen ausführen, weil ein einziger ungültiger Eintrag das Protokoll für alle Clients dauerhaft unprüfbar macht.
- [ ] **Step 5: Tests grün** (`cargo test -p sshcraft-proto`, `cargo clippy -p sshcraft-proto -- -D warnings`)
- [ ] **Step 6: Commit** `git commit -m "sshcraft-proto: Anmeldung an die Server-Adresse gebunden, Schlüsselreste getilgt, Tests fürs Geräteprotokoll"`

---

### Task 12: Eine Instanz, strengere CSP, Icons, Doku und Abschluss

**Files:**
- Modify: `app/src-tauri/src/lib.rs`, `Cargo.toml`, `tauri.conf.json`, `capabilities/default.json`, `app/src-tauri/icons/*`, `app/src/i18n/no-hardcoded-text.test.ts`, `README.md`, `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md`
- Create: `docs/shortcuts.md`

- [ ] **Step 1: Eine Instanz.** `tauri-plugin-single-instance` 2.5.5 als erstes Plugin registrieren; ein zweiter Start holt das vorhandene Hauptfenster nach vorn (`unminimize`, `show`, `set_focus`) und beendet sich.
- [ ] **Step 2: CSP.** Ziel: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`. Mit `npm run tauri build` bauen, starten, ein Terminal öffnen, die Konsole der WebView auf CSP-Verstöße prüfen. Braucht xterm.js eingefügte `<style>`-Elemente, bleibt bei `style-src` zusätzlich `'unsafe-inline'`, und der Grund steht als Kommentar in `README.md` unter Sicherheit; alle anderen Richtlinien gelten in jedem Fall.
- [ ] **Step 3: Icons** aus `app/src/assets/logo.svg`: `npm run tauri icon src/assets/logo.svg`. Nimmt die CLI kein SVG an, erst eine 1024-px-PNG erzeugen (`npx @resvg/resvg-js-cli --fit-width 1024 src/assets/logo.svg logo-1024.png`) und daraus die Icons; die PNG kommt nicht ins Repository.
- [ ] **Step 4: Wächter scharf.** `PENDING` im Test aus Task 1 ist leer und wird entfernt; der Test läuft über alle `.tsx`-Dateien.
- [ ] **Step 5: Doku.** `docs/shortcuts.md` mit der Tastenbelegung aus Task 7 (Englisch, wie die übrige Doku im Repository); README: eine Zeile zu den Sprachen und ein Link auf `docs/shortcuts.md` in der Tabelle "Dokumentation". Fahrplan: Teilplan 4 auf "fertig, auf `main`".
- [ ] **Step 6: Volle Prüfung** (das Repository hat noch keine laufende CI): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, in `app/`: `npm run typecheck`, `npm test`, `npm run build`. UTF-8 und Umlaute in `de.json` prüfen.
- [ ] **Step 7: Installer bauen** (`npm run tauri build`), Pfad notieren.
- [ ] **Step 8: Prüfung von Hand durch Moritz** (nicht automatisierbar): zweiten Start versuchen (Fenster kommt nach vorn); Sprache AUTO, Englisch, Deutsch umschalten; Themes und Schrift wechseln, Terminal übernimmt sie sofort; Strg+W und Strg+R in `bash` wirken in der Shell; Strg+Umschalt+P öffnet die Palette, ein Host daraus verbindet; Tab mit offener Fingerprint-Frage schließen (Dialog verschwindet); Tab während Windows Hello schließen (Dialog verschwindet); Proton Pass beenden und ssh-config-Vorschau öffnen ("Proton Pass nicht erreichbar"); einen Host anlegen, verbinden, er steht unter "Zuletzt verbunden".
- [ ] **Step 9: Commit** `git commit -m "Eine Instanz, strengere CSP, eigenes Logo, Doku zu Tastenkürzeln"`
