# SSHCraft: Neuaufbau nach den Konventionen der Craft-Apps

Stand: 14.11.2026, Entwurf zur Freigabe.

Dieser Entwurf ersetzt Abschnitt 3 (Technik) und die Teile von Abschnitt 10 (Anbindung für
KI-Agents), die den Transport betreffen, aus [2026-11-11-sshcraft-design.md](2026-11-11-sshcraft-design.md).
Alles andere dort gilt weiter, soweit hier nichts anderes steht: Ziele, Funktionsumfang nach
Termius-Vorbild, Datenmodell, Schlüsselquellen, Konten, Sync, Teilen, Rechte für Agents und
Sicherheit auf dem Gerät.

## 1. Worum es geht und warum

SSHCraft soll später zur Familie der Craft-Apps von storytold gehören (GridCraft, WordCraft,
DeckCraft, PdfCraft, PhotoCraft und die anderen "ArtCraft Crafting Apps"). Deshalb übernimmt es
deren Architektur und Konventionen wörtlich: dieselben geschichteten Crates, dieselbe Engine nach
dem Grundsatz "everything is a command", dieselbe egui-Oberfläche mit Design-Tokens, denselben
Control-Channel, denselben MCP-Server, dieselben Werkzeuge und dieselbe Paketierung, dazu die
House rules aus `storytold/craft-contrib`.

Moritz hat am 14.11.2026 den **Neuaufbau** gewählt, nicht den schrittweisen Umbau: ein neues
Workspace-Gerüst nach Craft-Vorlage, die tragfähigen Rust-Crates ziehen hinein, die Oberfläche
entsteht neu in egui, Tauri und React verschwinden zu Beginn.

**Vorlagen**, wenn sie sich widersprechen, in dieser Reihenfolge:

1. `storytold/craft-contrib` (`standards/`, öffentlich): House rules, Lizenzliste.
2. `storytold/pdfcraft` und `storytold/photocraft`: die strengsten Regeln zu feindlicher Eingabe
   und Agent-Steuerung, also am nächsten an einem SSH-Client.
3. `storytold/gridcraft`, `storytold/wordcraft`, `storytold/deckcraft`.
4. Geteilte Bausteine: `storytold/craft-libs` (MIT OR Apache-2.0) und
   `storytold/craft-ai-integrations` (Apache-2.0).

## 2. Ziele, Nicht-Ziele, Anti-Ziele

### Ziele

1. Repository-Aufbau, Regeln und Werkzeuge wie in den Craft-Apps (Abschnitte 4 bis 12).
2. **Funktionsgleichheit mit `main` bei `38ab865`** (Teilpläne 1 bis 15 aus
   `plans/2026-11-11-sshcraft-roadmap.md`): Was die Tauri-Fassung kann, kann die egui-Fassung
   auch. Maßstab sind die Doku unter `docs/` (agents, connections, forwarding, hosts, keys,
   protocol, security, server, server-api, session-logs, shortcuts, snippets, sync, workspaces)
   und die Tests.
3. **Bestehende Daten und Server laufen weiter:** Datenbank (Schema 11), Einträge in der
   Anmeldeinformationsverwaltung unter `de.musiker15.sshcraft`, Sync-Protokoll und Server-API
   bleiben kompatibel. Wer die Tauri-Fassung benutzt hat, startet die egui-Fassung und hat seine
   Hosts, Schlüssel, Protokolle und Sync-Konten.
   **Der Wechsel geht nur in eine Richtung** [Claudes Entscheidung]: Die egui-Fassung startet
   nicht, solange die Tauri-Fassung läuft (sie erkennt deren Einzelinstanz-Sperre), übernimmt
   beim ersten Start die Einstellungen in `config.toml` und vermerkt die Übernahme in der
   Datenbank. Das MSI entfernt die NSIS-Installation samt altem `sshcraft.exe`. Zwei Sync-Engines
   mit demselben Geräteschlüssel darf es nie geben.
4. Windows zuerst lauffähig. macOS, Linux und FreeBSD bauen und testen in der CI; der bisherige
   Teilplan 16 geht im Neuaufbau auf.
5. `cargo xtask ci` grün: Formatierung, clippy mit `-D warnings`, Tests, Assets, Schichten,
   Lizenzen, wasm-Check.

### Nicht-Ziele

- Neue Funktionen über Teilplan 15 hinaus **in den Teilplänen R1 bis R7**, außer denen, die die
  Craft-Konventionen mitbringen (Befehlsregister, Control-Channel mit `ui.screenshot`,
  Anbindung an den ArtCraft Launcher) und den Anforderungen aus Abschnitt 17. Neue Funktionen
  aus dem Marktvergleich kommen in eigenen Teilplänen danach (Abschnitt 16).
- **Android und iOS** (Fahrplan 18 bis 20). Die Craft-Apps zielen auf Desktop und Web. Nach dem
  Neuaufbau wird neu bewertet; egui läuft auf Android, iOS ist offen. Der Zweig `spike/android`
  bleibt liegen.
- **SSH im Browser.** Eine Web-Fassung (`apps/sshcraft-web`) entsteht nicht im Neuaufbau. SSH
  braucht TCP, im Browser ginge es nur über ein Relay; das ist ein eigenes Vorhaben. Der
  wasm-Check gilt trotzdem für die Crates ohne Netz und Dateisystem (Abschnitt 6.4).
- SFTP in der ersten Version. Moritz, 14.11.2026: SFTP kommt in den Plan (Teilplan S1), muss
  aber nicht in den ersten Release.
- Die Aufnahme in die Organisation storytold. Das entscheidet storytold; der Neuaufbau macht
  SSHCraft nur aufnahmefähig.

### Anti-Ziele (ein Fehlschlag, auch wenn es technisch läuft)

- Ein Agent erreicht über einen neuen Weg (Control-Channel, MCP ohne Fenster, Verbindung über
  den Launcher) mehr als heute, ohne dass der Benutzer es in den Einstellungen freigibt; nie
  einen privaten Schlüssel, ein Passwort, einen ausgeblendeten Host, eine Aktion der Stufe
  "fragen" ohne Rückfrage im Fenster, oder eine Rückfrage, die er selbst bestätigt.
- Eine Sicherheitsregel aus den Teilplänen 1 bis 15 geht verloren, besonders die Befunde aus den
  Reviews von Teilplan 14 und 15 (Abschnitt 10).
- Tauri, ein Webview, handgeschriebenes JavaScript oder TypeScript, AGPL-Code oder eine
  Abhängigkeit außerhalb der Lizenzliste bleibt im Repository.
- Ein Panic im Nicht-Test-Code oder ein `unsafe` außerhalb der benannten Ausnahme.
- Datenverlust beim Wechsel von der Tauri-Fassung.

## 3. Ansätze

| Weg | Was | Abwägung | Entscheidung |
|---|---|---|---|
| A: schrittweise | Erst Regeln und Werkzeuge, dann Engine hinter Tauri, dann egui Seite für Seite, Tauri zuletzt raus | Das Programm läuft nach jedem Schritt; eine Zeit lang zwei Oberflächen und Doppelarbeit an der Tauri-Brücke | von Moritz verworfen |
| B: Neuaufbau | Neues Gerüst nach Craft-Vorlage, Kern-Crates ziehen hinein, Oberfläche neu in egui, Tauri sofort weg | Schneller sauber, keine Brücke; lange keine benutzbare neue Oberfläche, die 392 vitest-Tests fallen weg | **gewählt** |
| C: Tauri als Ausnahme | Konventionen übernehmen, Oberfläche bleibt Tauri | Widerspricht der House rule "Rust only for native Craft apps" und damit dem Ziel | verworfen |

Folgen aus B:

- Bis Teilplan R5 (Abschnitt 13) gibt es keine benutzbare neue Oberfläche. Die Tauri-Fassung
  bleibt so lange über den gebauten Installer `SSHCraft_0.1.0_x64-setup.exe` und den Git-Stand
  `38ab865` (Tag `tauri-final`) benutzbar.
- Die vitest-Tests fallen weg. Was sie an Verhalten prüfen (nicht an Darstellung), wird in
  Engine-Tests oder egui_kittest-Tests neu belegt. Die Logik dahinter steht in `app/src/**` der
  Historie (`tauri-final`).

## 4. Repository-Aufbau

```text
sshcraft/
  AGENTS.md  CLAUDE.md  README.md  ROADMAP.md  SECURITY.md  GLOSSARY.md
  ATTRIBUTION.md  NOTICE  LICENSE-MIT  LICENSE-APACHE
  Cargo.toml  Cargo.lock  clippy.toml  rustfmt.toml  deny.toml
  .cargo/config.toml        xtask-Alias
  .config/nextest.toml
  .coderabbit.yaml  sonar-project.properties  .gitattributes  .gitignore  .protonignore
  .github/workflows/*.yml  .github/dependabot.yml
  apps/sshcraft             Desktop-App (eframe)
  apps/sshcraft-cli         Kommandozeile und MCP
  apps/sshcraft-server      Sync-Server
  crates/<name>             Paketname sshcraft-<name>
  xtask/
  packaging/{windows,macos,linux,freebsd,docker}
  assets/                   App-Symbol, Icons; jede Datei mit Zeile in ATTRIBUTION.md
  contributors/{people.toml,contributors.json}
  docs/
  plan/                     gitignored Notizbuch (Craft-Konvention)
```

- Workspace: `resolver = "3"`, `members = ["crates/*", "apps/*", "xtask"]`, `edition = "2024"`,
  `license = "MIT OR Apache-2.0"`, `rust-version` wie die Familie,
  `repository = "https://github.com/musiker15/sshcraft"`.
- Lints wie gridcraft: `[workspace.lints.rust] unsafe_code = "forbid"`; clippy
  `cognitive_complexity`, `too_many_arguments`, `wildcard_imports` auf `deny`, `dbg_macro` und
  `todo` auf `warn`. Jedes Crate hat `[lints] workspace = true`, jedes Produktions-Crate das
  Attribut `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
  clippy::unimplemented, clippy::todo, clippy::unreachable)]`. `clippy.toml` erlaubt das in Tests.
- Profile wie gridcraft. **`panic` bleibt `unwind`** (heute steht `abort` im Release-Profil; das
  verträgt sich nicht mit dem Absturzschutz in Abschnitt 6.3).
- egui 0.37 mit demselben `[patch.crates-io]` auf `storytold/egui` wie die Familie; alle
  craft-libs-Crates aus einer gemeinsamen Revision.
- `docs/superpowers/` verschwindet am Ende des Neuaufbaus aus dem Baum, die Historie behält es.
  Was davon weiter gilt, steht dann englisch in `docs/architecture.md`, `docs/decisions.md` und
  `docs/security.md`. Neue Pläne liegen ab dann in `plan/`.

## 5. Schichten

`cargo xtask layers` prüft die Tabelle (Muster: `xtask/src/layers.rs` der Familie). Ein Crate
hängt nur an tieferen Schichten. Nichts unter L6 hängt an egui, eframe, winit oder rfd.

| Schicht | Crate | Herkunft | Aufgabe |
|---|---|---|---|
| L0 | `proto` | `sshcraft-proto`, unverändert | Konto- und Geräteschlüssel, Geräteprotokoll, Bereichsschlüssel, signierte und verschlüsselte Datensätze, Rückfallschutz |
| L0 | `platform` | neu, Teile aus `app/src-tauri` | Ordner, Dateien nur für den Besitzer (wie `pdfcraft_platform::owner_only`, unter Windows über `icacls`, kein `unsafe`), Betriebssystem-Erkennung, Login-Shell |
| L1 | `model` | aus `sshcraft-core` (`model`, `policy`, `keytypes`, `history`, Typen der Snippets) | reine Daten: Hosts, Gruppen, Identitäten, Schlüssel-Metadaten, Regeln, Rechte |
| L2 | `store` | aus `sshcraft-core` (`store`, `vault`, `credential_store`, `sync_store`, `share_store`, `session_log`, `logtext`, `agent`) | SQLite (Schema 11 bleibt), Tresor, Hauptschlüssel in der Anmeldeinformationsverwaltung |
| L2 | `import` | aus `sshcraft-core` (`sshconfig`, `import/*`, `export`, `authorized_keys`) | Import und Export |
| L2 | `term` | neu | Terminal-Emulation ohne Toolkit (Abschnitt 7.3) |
| L3 | `ssh` | `sshcraft-ssh` | russh, Tunnel, HTTP- und SOCKS5-Proxy, Agent-Weiterleitung |
| L3 | `local` | `sshcraft-local` | PTY und serielle Verbindung |
| L3 | `sync` | `sshcraft-sync` | Client-Sync, Teilen, Geräte (`server` nur als dev-dependency) |
| L3 | `server` | `sshcraft-server` als Bibliothek | Sync-Server (axum, SQLite, PostgreSQL) |
| L4 | `fido` | `sshcraft-fido` plus `app/src-tauri/src/hello.rs` | FIDO2, Windows Hello; die eine `unsafe`-Ausnahme (Abschnitt 8) |
| L5 | `engine` | neu, aus `app/src-tauri/src` | `Session`, Befehlsregister, Agent-Prüfung, Verbindungen, Sitzungen, Aufzeichnung, Weiterleitungen, Workspaces, Snippet-Ausführung |
| L6 | `ui-egui` | neu | Oberfläche und Control-Channel |
| L6 | `mcp` | aus `crates/sshcraft-cli/src/mcp.rs` | MCP-Server über stdio, Backend ohne Fenster oder an der laufenden App |
| L7 | `apps/sshcraft`, `apps/sshcraft-cli`, `apps/sshcraft-server` | | Programme |
| – | `xtask` | neu | `ci`, `layers`, `assets`, `deny`, `wasm`, `version`, `stats`, `parity` |

- `sshcraft-agent` entfällt. Named Pipe und Pipe-Protokoll ersetzt der Weg der Familie
  (Abschnitt 9); damit fallen auch dessen `unsafe`-Blöcke weg.
- Die Aufteilung von `sshcraft-core` ist ein Vorschlag. Bedingungen: keine Schicht greift nach
  oben, `proto` und `model` bleiben wasm-fähig, `store` liest Schema 11 ohne Migration.

## 6. Engine: everything is a command

### 6.1 Befehle

- `crates/engine/src/cmd/<bereich>.rs`, jede Datei mit `specs()`, registriert in `cmd/mod.rs`.
  `CommandSpec { id, label, menu, shortcut, params, enabled, run, journal, undoable, agent }`
  nach dem Muster von gridcraft. `agent` ist die Agent-Kategorie aus Teilplan 15 oder `None`
  (für Agents nie erreichbar).
- Die rund 110 Tauri-Befehle aus `app/src-tauri/src/commands/*.rs` werden Engine-Befehle. Ids
  folgen der Seiten- und Menüstruktur: `host.*`, `group.*`, `key.*`, `identity.*`, `session.*`,
  `workspace.*`, `forward.*`, `snippet.*`, `log.*`, `sync.*`, `share.*`, `device.*`,
  `import.*`, `settings.*`, `agent.*`, dazu Grundbefehle wie `selection.set` und `view.page`.
  Eine ausgelieferte Id bleibt stabil.
- Oberfläche, CLI, Control-Channel und MCP rufen nur `Session::execute(caller, id, params)`.
  Was nur Fenster und Ansicht betrifft (Zoom, Seitenleiste ein- und ausklappen), sind UI-Befehle
  in `ui-egui`.
- **Aufrufe aus einem Programm öffnen nie einen Dialog.** Leere Parameter heißen Vorgaben oder
  ein Fehler; nur der Aufruf aus dem Menü öffnet einen Dialog (`UiRequest::Dialog`).
- `engine/src/catalog.rs` beschreibt jeden Befehl für Menü und Befehlspalette;
  `cargo xtask parity` erzeugt daraus `docs/parity-checklist.md`, gemessen an
  `docs/target-app-parity.md` (Termius).
- Terminal-Ausgabe läuft nicht über das Befehlsregister, sondern über einen Kanal von der Engine
  zur Oberfläche. Befehle sind für Aktionen; Eingaben ins Terminal sind ein Befehl
  (`session.send`).

### 6.2 Aufrufer und Agent-Prüfung

- Jeder Aufruf trägt einen `Caller`: `User` (Oberfläche, Menü, Tastenkürzel), `Agent { name,
  via }` (MCP, Launcher-Verbindung, CLI) oder `Control` (Control-Channel).
- Die Prüfung aus Teilplan 15 (`app/src-tauri/src/agents/gate.rs`) zieht in die Engine und gilt
  für jeden Aufruf außer `User`: Rechte je Kategorie (erlaubt, fragen, gesperrt), ausgeblendete
  Hosts und Gruppen, versiegeltes Protokoll vor der Ausführung (Listen ausgenommen), nie
  Schlüsselmaterial in einer Antwort, Zugangsdaten nicht umlenkbar.
- `Control` gilt für die Prüfung als Agent. `--control` ist kein Freibrief für Befehle.
- "fragen" braucht die Rückfrage im SSHCraft-Fenster (120 s, wie heute).
- **Was Agents erreichen** [Claudes Entscheidung]: Jeder Befehl hat eine Kategorie oder `None`.
  Die Kategorien der heutigen 15 Agent-Aktionen (Hosts, Gruppen und Schlüssel auflisten,
  speichern, löschen, erzeugen, importieren; `exec`; Sitzungen) behalten ihre heutigen Vorgaben.
  Alle weiteren Kategorien (Portweiterleitung, Protokolle lesen, Workspaces, Snippets, dazu die
  schon vorhandenen Teilen und Sync) beginnen auf "gesperrt" und öffnen sich nur, wenn der
  Benutzer sie in den Einstellungen freigibt. `None` und damit für Agents nie erreichbar sind:
  Befehle, die Geheimnisse liefern oder anzeigen, `import.*` mit einem Pfad aus Agent-Parametern,
  Einstellungen der Agent-Rechte selbst, Konto, Geräte-Sperre und Schlüsseltausch.

### 6.3 Absturzschutz

- `Session::execute` fängt einen entkommenen Panic ab (`catch_unwind`), stellt den Zustand wieder
  her und meldet einen Fehler.
- Wie gridcraft: `tests::every_command_survives_empty_params` über alle Befehle und ein
  Fuzz-Test über zufällige Befehlsfolgen.

### 6.4 wasm

`cargo xtask wasm` prüft `proto` und `model` mit `wasm32-unknown-unknown`. Die übrigen Crates
brauchen Netz, Dateisystem oder Betriebssystem und sind ausgenommen, auch `term`:
`alacritty_terminal` bringt immer seine `tty`- und `event_loop`-Module mit [Claudes Entscheidung]. Die Ausnahme steht
mit Grund in `AGENTS.md`; sie weicht von "never break wasm (L0 bis L6)" der Familie ab, weil es
keine Web-Fassung gibt.

## 7. Oberfläche (egui)

### 7.1 Grundsätze

- `crates/ui-egui` ist dünn: Seiten lesen den Zustand der Engine und handeln über
  `app.run(id, params)`. Der UI-Zustand liegt in `ui-egui/src/state.rs` (serde), damit der
  Control-Channel ihn lesen und setzen kann.
- Farben und Radien kommen nur aus `theme::Tokens`, jeder Wert hell und dunkel.
  `theme::contrast_tests` hält jedes Paar aus Vorder- und Hintergrund auf WCAG 2 (4,5:1 für
  Text, 3:1 für Symbole). Widget-Zustände über `craft_ui::theme::WidgetPalette`.
- Gemeinsame Widgets in `widgets.rs`. Keine Farbe im Widget fest verdrahtet. Eine Beschriftung
  läuft nie über ihre Zeile (`elided_label` mit Tooltip).
- Bausteine aus craft-libs, nur wo sie einen Zweck haben: `craft-ui` (Icons, Tabs, Bereiche,
  Docking, Theme-Helfer), `craft-settings` (`config.toml`), `craft-i18n` (Englisch und Deutsch,
  Sprache AUTO wie heute), `craft-keymap` (Tastenkürzel, eigene Belegung), `craft-window`,
  `craft-dialogs`, `craft-flags`, `craft-menu` und `craft-quit` (macOS). `craft-update` erst,
  wenn es Releases gibt.
- Schriften über `CRAFT_FONTS_DIR` (`storytold/craft-fonts`), ohne sie Systemschriften und die
  Vorgaben von egui. Nie eine Schriftdatei im Repository.
- Icons im Code gezeichnet oder Lucide (ISC), jede Datei mit Zeile in `ATTRIBUTION.md`. Nichts
  von Termius außer Funktionsnamen.

### 7.2 Aufbau

Wie heute nach Termius-Vorbild (Design 2026-11-11 Abschnitt 9, Teilpläne 4 und 11):
Seitenleiste (Hosts, Schlüssel, Snippets, Portweiterleitung, Protokolle, Einstellungen), Tabs,
Befehlspalette, Neuer-Tab-Seite, Workspaces mit Split und Fokus für 2 bis 16 Sitzungen,
Broadcast, Statuspunkte, Rec-Markierung, Rückfrage-Fenster für Agents. Die heutigen Themes und
Schriftwahlen werden Token-Sätze.

```text
+-------------+----------------------------------------------+
| Hosts       | [web-01] [db-02] [+]                         |
| Schlüssel   | +---------------------+--------------------+ |
| Snippets    | | Terminal            | Terminal           | |
| Weiterl.    | |                     |                    | |
| Protokolle  | +---------------------+--------------------+ |
| Einstell.   | Statusleiste                                 |
+-------------+----------------------------------------------+
```

### 7.3 Terminal

- Neues Crate `term` (L2, ohne Toolkit) auf `alacritty_terminal` (Apache-2.0): VT-Parser,
  Gitter, Scrollback, Auswahl, OSC 52, Hyperlinks, dazu Suche und Marken für Lesezeichen.
- `ui-egui/src/terminal/` zeichnet das Gitter mit dem egui-Painter (Glyphen-Cache, Cursor,
  Auswahl, Unterstreichung, doppelt breite Zeichen) und nimmt Tastatur, IME und Maus an.
- Flusskontrolle wie heute: Die Engine liest erst weiter, wenn das Terminal genug verarbeitet
  hat.
- Die Ansicht der Sitzungsprotokolle nutzt `term` schreibgeschützt.
- Die Autovervollständigung aus Teilplan 13 ist ein Overlay über dem Terminal.

### 7.4 Prüfen

egui_kittest-Tests, `cargo run -p sshcraft-ui-egui --example snapshot -- out.png` ohne Fenster,
Screenshots über `ui.screenshot`. Vor- und Nachher-Bilder gehören in die Beschreibung eines Pull
Requests, nie ins Repository.

## 8. Nie abstürzen, kein `unsafe`

- Regeln wie in den House rules (Rust) und in gridcraft: kein `unwrap`, `expect`, `panic!`,
  `unreachable!`, `todo!` oder `unimplemented!` außerhalb von Tests. Heute stehen rund 450
  `unwrap` und `expect` im Nicht-Test-Code; sie verschwinden beim Umzug.
- Eingaben sind feindlich: Dateien beim Import, Antworten von Servern, Terminal-Ausgabe mit ihren
  Escape-Folgen, Parameter aus MCP und Control-Channel. Index nur über `get()`, Rechnen mit
  `checked_*` oder `saturating_*`, Speicher nach Eingabegröße begrenzen, Rekursion begrenzen.
- **Einzige `unsafe`-Ausnahme: `crates/fido`** (WebAuthn und Windows Hello über `webauthn.dll`),
  nach dem Muster von `photocraft-tablet`: `unsafe_code = "deny"` im Crate, erlaubt nur im
  Plattform-Modul, jeder Block mit `SAFETY:`-Kommentar, benannt in `AGENTS.md`. Deckt ein
  gepflegter sicherer Wrapper die nötigen Aufrufe ab, wird er genommen und die Ausnahme fällt.

## 9. Steuerung von außen

### 9.1 Control-Channel

- Wie in der Familie, Vorlage `docs/control-protocol.md` von pdfcraft und photocraft: nur mit
  der Startoption `--control`, nur Loopback, ein Token je Start aus 256 Zufallsbits in einer
  Datei nur für den Besitzer (`platform::owner_only`), erste Zeile `auth`, danach JSON-Zeilen,
  Grenzen für Zeilenlänge, wartende Verbindungen und Eingabewarteschlangen. Nie standardmäßig
  an, nie eine breitere Adresse, nie der Token im Log.
- Methoden: `engine.execute`, `engine.commands`, `engine.journal` (ohne Passwörter, Passphrasen
  und Schlüssel), `window.list`, `ui.inspect`, `ui.click`, `ui.drag`, `ui.move`, `ui.key`,
  `ui.text`, `ui.screenshot`, `ui.set`, `ui.resize`, `ui.focus`, `app.quit`, dazu
  `terminal.read` (sichtbarer Text eines Bereichs, für Tests).
- `--control` ist ein Entwicklerwerkzeug: Wer den Token hat, klickt und tippt wie der Benutzer,
  kann also eine Rückfrage bestätigen und ein Terminal fotografieren. Das steht deutlich in
  `docs/control-protocol.md` und `SECURITY.md`. `engine.execute` über diesen Weg läuft trotzdem
  als `Caller::Control` durch die Prüfung.

### 9.2 Agent-Verbindung über den ArtCraft Launcher

- Wie photocraft (`docs/mcp.md` dort): hinter dem Feature-Flag `AI-MCP` aus `craft-flags`. Der
  Schalter "Let AI agents use Craft apps" im ArtCraft Launcher ist die Zustimmung. SSHCraft liest
  `ArtCraft/mcp/apps.json` über `craft-ai-agents` (craft-ai-integrations), bedient Agents auf
  Loopback mit einem Token aus seinem Einstellungsordner und veröffentlicht Port und Token in
  `agent-connection.json` (nur für den Besitzer).
- Diese Verbindung kennt nur `engine.execute`, `engine.commands` und lesende Methoden, **nie**
  `ui.*`-Eingaben oder Screenshots. Alles läuft als `Caller::Agent` durch die Prüfung.
- Der eigene Schalter aus Teilplan 15 ("Agents erlauben", Vorgabe aus) wird "Allow Agent
  Control". Ist er aus, werden Agents abgewiesen, egal was der Launcher sagt.
- **Ohne das Flag `AI-MCP`** (der Normalfall, solange SSHCraft nicht in der Liste des Launchers
  steht) entscheidet der eigene Schalter allein: Ist er an, startet SSHCraft den Loopback-Dienst
  und veröffentlicht `agent-connection.json`, wie photocraft ohne Flag mit "Allow Agent Control"
  und "Control Port". So bleibt der heutige Weg (`claude mcp add …`) erhalten. **Mit dem Flag**
  müssen Launcher-Schalter und eigener Schalter beide an sein [Claudes Entscheidung].
- SSHCraft schreibt keine Konfiguration eines MCP-Clients; das macht nur der Launcher.

### 9.3 MCP

- `crates/mcp`: von Hand geschriebenes JSON-RPC über stdio (Protokoll 2025-06-18, ältere Clients
  in ihrer Fassung), übernommen aus `crates/sshcraft-cli/src/mcp.rs` mit parallelen Aufrufen und
  Abbrechen wie heute. Ein `Backend`-Trait mit `Remote` (laufende App über
  `agent-connection.json` oder `--bridge`); eine Engine im Prozess gibt es nur in Tests.
- Werkzeuge: `execute_command`, `list_commands`, `list_windows`, dazu die heutigen aus
  `docs/agents.md` unter ihren heutigen Namen. Jedes Werkzeug ist eine dünne Hülle um
  Engine-Befehle.
- `sshcraft-cli mcp` arbeitet im offenen Fenster, wenn Agents erlaubt sind. Ist SSHCraft zu,
  entscheidet `--when-closed <launch|error>`, Vorgabe `error` wie heute (Meldung "SSHCraft
  läuft nicht", Exit-Code 3); `launch` startet die App wie ein Doppelklick und wartet auf sie.
  `--bridge` arbeitet an einer `--control`-App.
- **Kein Betrieb ohne Fenster** [Claudes Entscheidung, Abweichung von photocraft, steht mit Grund
  in `AGENTS.md`]: Die Rückfrage und die Abfrage von Windows Hello vor gespeicherten Schlüsseln
  brauchen das Fenster, und Datenbank und Tresor dürfen nur ein Prozess öffnen. Ein `Headless`
  ohne diese Prüfungen wäre schwächer als heute (Anti-Ziel). Das `Backend`-Trait bleibt, damit
  Tests eine Engine im Prozess nutzen können.
- **Nur ein Prozess öffnet die Daten:** Die App hält eine exklusive Sperre auf ihren
  Datenordner; ein zweiter Start übergibt an die laufende Instanz. Die CLI öffnet die Datenbank
  nie selbst.
- Das MCP startet nie von selbst, wird nie registriert und lauscht auf keinem Port.

### 9.4 Kommandozeile

`apps/sshcraft-cli`, Programmname `sshcraft-cli` wie in der Familie (heute heißt es `sshcraft`):
`commands`, `run --cmd 'id={json}'`, `send`, `ui …`, `mcp`, dazu die heutigen Befehle aus
Teilplan 15 als Unterbefehle. Eine bestehende Registrierung `claude mcp add sshcraft --
sshcraft.exe mcp` muss danach neu angelegt werden; die Doku sagt es.

## 10. Was aus den Teilplänen 1 bis 15 mitkommen muss

Diese Regeln haben heute Tests. Sie ziehen mit Test um oder bekommen einen neuen, und der Plan
führt sie als Abnahme-Liste. Quellen: die Teilplan-Dateien unter `docs/superpowers/plans/`, die
Tests unter `crates/*/tests`, `crates/*/src/*tests*.rs` und `app/src-tauri/src/*_tests.rs`.

- Ein geänderter Server-Fingerprint bricht immer ab, auch beim Wiederverbinden (T1).
- Fehlt der Hauptschlüssel, obwohl Schlüssel existieren, entsteht kein neuer (T1).
- Ende-zu-Ende-Verschlüsselung, Rückfallschutz, Geräte-Sperre mit Schlüsseltausch, Entziehen
  einer Freigabe mit Schlüsseltausch (T3, T6, T7).
- Aufzeichnung nur der Ausgabe, nie der Tastatur; Aufbewahrung löscht auch am Server; Log-Sync
  nur im persönlichen Bereich; Suche hält keine Datenbanksperre (T14).
- Alle Befunde aus dem Review von T15: `exec` liefert Fehlerausgabe und Exit-Code; `save_host`
  überschreibt nichts und lenkt keine Zugangsdaten um; Sprung-Hosts verraten keine
  ausgeblendeten Hosts; `delete_group` löscht nichts Ausgeblendetes; ein Abbruch wird
  protokolliert; das Protokoll lässt sich nicht fluten; ein Klick trifft nicht die nächste
  Rückfrage; Sitzungen enden mit dem Agent.

## 11. Doku, Regeln, Lizenz

- **`AGENTS.md`:** die Blöcke `core` und `rust` aus `storytold/craft-contrib/standards/`
  wörtlich mit ihren Markierungen (`<!-- craft-house-rules:start … sha256=… -->`), dann der Teil
  für SSHCraft nach dem Muster von gridcraft: Start every session here, Assets, Never crash,
  Non-negotiables (Clean-Room gegenüber Termius: nur beobachten; Everything is a command; keine
  Dialoge bei Programmaufrufen; Layering; dünne Oberfläche; Rust only; Quality gates), Running
  and looking at the app, Roadmap, Contributor credits, CI workflows, die `unsafe`-Ausnahme, die
  wasm-Ausnahme und die Sicherheitsregeln für Agents.
- **`CLAUDE.md`** enthält nur `@AGENTS.md`.
- **README** nach dem Muster der Familie, englisch, und klein nach der README-Regel von Moritz:
  Einzelheiten stehen in `docs/` und werden verlinkt.
- **`SECURITY.md`** nach der Vorlage von wordcraft und photocraft, mit dem Umfang von SSHCraft:
  Server liest nie mit, Agents, Control-Channel.
- **`ATTRIBUTION.md`, `NOTICE`** (Lizenztexte mit Hinweispflicht), **`contributors/`**.
- **`ROADMAP.md`** (eine Seite), dazu `docs/roadmap.md`, `docs/gaps.md`,
  `docs/target-app-parity.md` (Termius), `docs/ui-parity.md`, `docs/localization-parity.md`,
  `docs/architecture.md`, `docs/decisions.md`, `docs/control-protocol.md`, `docs/mcp.md`,
  `docs/releasing.md`. Die heutigen Feature-Dokus bleiben und werden angepasst.
- **Lizenz:** `LICENSE` (AGPL-3.0-only) wird durch `LICENSE-MIT` und `LICENSE-APACHE` ersetzt.
  Moritz ist alleiniger Autor aller 225 Commits und darf das. Ältere Stände bleiben in der
  Historie unter AGPL.
- **Sprache im Repository:** Englisch, auch die Commit-Nachrichten, eine Aufgaben-Id je Commit
  (`R2.3: host commands`), wie in der Familie und wie bei msk_core-vscode. Die Oberfläche bleibt
  englisch und deutsch.
- Keine ArtCraft-Markendateien (`docs/brand/`), solange storytold SSHCraft nicht aufgenommen hat.

## 12. Werkzeuge, CI, Pakete

- **`xtask`:** `ci` (fmt, clippy, Tests, assets, layers, deny, wasm), `layers`, `assets` (jede
  Asset-Datei hat eine Zeile in `ATTRIBUTION.md`), `deny` (cargo-deny mit der Lizenzliste der
  House rules; `allow-git` für craft-libs, den egui-Fork und craft-ai-integrations), `wasm`,
  `version`, `stats`, `parity`.
- **CI:** `.github/workflows/ci.yml` ruft `storytold/craft-common-actions/.github/workflows/gate.yml@main`,
  dazu nach dem Muster von gridcraft `house-rules.yml`, `packaging-lint.yml`, `freebsd.yml`,
  `windows-arm64.yml`, `supply-chain.yml` und `release.yml`. Das Repository ist privat; die
  geteilten Workflows lassen sich von dort aufrufen, verbrauchen aber die Freiminuten des Kontos
  (macOS-Läufe zählen mehrfach). Solange das knapp wird, bleibt `cargo xtask ci` lokal das Tor
  und die CI läuft für Pull Requests.
- **Pakete** unter `packaging/` nach gridcraft: Windows MSI über WiX mit `sign.ps1` und der
  Installationsrichtlinie aus craft-libs (`docs/installation-policy.md`), macOS `.app` und DMG,
  Linux deb und rpm (nfpm) und Flatpak, FreeBSD. Der Sync-Server als Linux-Programm und
  Docker-Image (aus Teilplan 5 und 17).
- **App-Kennung** bleibt `de.musiker15.sshcraft` (Anmeldeinformationsverwaltung, Ordner,
  Flatpak-Id). Die Familie nutzt `ai.storyteller.<app>`; bei einer Aufnahme würde das
  umgezogen.
- Releases über `release.yml` der Familie. Signierschlüssel und GitHub-Einrichtung erst mit
  einem Remote.

## 13. Reihenfolge

| Nr. | Teilplan | Ergebnis |
|---|---|---|
| R1 | Gerüst und Regeln | Tag `tauri-final` auf `38ab865`; neuer Workspace, Lizenzwechsel, `AGENTS.md` und Doku-Gerüst, xtask (`ci`, `layers`, `assets`, `deny`, `wasm`), Lints; `app/` und `sshcraft-agent` entfernt; L0 bis L4 umgezogen, mit ihren Tests und ohne `unwrap` |
| R2 | Engine | `Session`, Befehlsregister, alle Befehle aus dem Tauri-Backend, Agent-Prüfung, Absturzschutz und Fuzz-Tests, Einstellungen über `craft-settings` mit Übernahme der alten Einstellungen |
| R3 | Terminal | beginnt mit einem kurzen Wegwerf-Versuch (große Ausgabe, `htop`, `vim`, IME); dann `term`, egui-Renderer, Snapshot-Beispiel |
| R4 | Oberfläche I | Fenster, Seitenleiste, Tabs, Befehlspalette, Tokens und Themes, Sprachen, Tastenkürzel; Hosts, Gruppen, Schlüssel, Identitäten, Import und Export, Verbinden |
| R5 | Oberfläche II | Workspaces, Broadcast, Portweiterleitung, Snippets und Autovervollständigung, Protokolle, Sync, Teilen, Geräte, Agent-Einstellungen und Rückfrage |
| R6 | Steuerung | Control-Channel, `mcp`, `sshcraft-cli`, Launcher-Anbindung; Abnahme-Liste aus Abschnitt 10 vollständig |
| R7 | Plattformen und Pakete | macOS, Linux, FreeBSD (früher Teilplan 16), `packaging/`, `docs/releasing.md`, CI-Dateien; `docs/superpowers/` aus dem Baum |

Danach F1 und F2 (Abschnitt 16.1), dann die erste Version V1 (früher Teilplan 17: signierte
Pakete, Prüfsummen, winget, Homebrew, Flathub, Server-Programm, Docker-Image). Was nach V1
kommt, steht in Abschnitt 16.2.

Jeder Teilplan wird wie bisher geplant und nativ umgesetzt, auf opus geprüft und über einen Pull
Request in `main` gebracht (House rule 8; das Repository `Musiker15/sshcraft` ist seit
14.11.2026 privat auf GitHub, ein Regelsatz erzwingt Pull Requests für `main`).

## 14. Risiken

- **Terminal in egui** ist der unsicherste Teil: Textdarstellung, IME und Geschwindigkeit bei
  großen Ausgaben. Deshalb beginnt R3 mit dem Wegwerf-Versuch, bevor die Oberfläche darauf baut.
- **Die Familie bewegt sich schnell:** egui-Fork, craft-libs in 0.x, craft-ai-integrations als
  "first draft". Revisionen werden gepinnt und nur gemeinsam angehoben.
- **Lizenzliste** [Claudes Entscheidung, nach dem Vorbild der Familie]: `deny.toml` erlaubt die
  Liste der House rules und führt Abweichungen nur als Ausnahme je Crate mit Begründung, wie
  pdfcraft (`[[licenses.exceptions]]` für `epaint_default_fonts` und `webpki-root-certs`).
  Erwartet sind `option-ext` (MPL-2.0, über `dirs` aus craft-libs; photocraft hat es ebenso im
  Baum), `unicode-ident` (Unicode-3.0) und `foldhash` (Zlib). Eigene Abhängigkeiten kommen nur
  aus der Liste: `serialport` (MPL-2.0) wird durch `serial2` (BSD-2-Clause OR Apache-2.0)
  ersetzt.
- **Keine Sicherheitsprüfungen der Familie für einen SSH-Client.** Die eigenen Tests aus
  Abschnitt 10 sind das Netz.
- Bis R5 keine benutzbare neue Oberfläche.

## 15. Dem Builder überlassen

- Die genaue Aufteilung von `sshcraft-core`, Modulnamen, die Befehls-Ids im Einzelnen.
- Ein anderer Terminal-Emulator mit erlaubter Lizenz, falls der Versuch in R3 gegen
  `alacritty_terminal` spricht.
- Welche craft-libs-Crates im Einzelnen genutzt werden.
- Grenzwerte (Zeilenlängen, Warteschlangen, Zeitlimits) nach dem Vorbild der Familie.

## 16. Funktionen über Teilplan 15 hinaus

Moritz, 14.11.2026: Funktionen, die andere SSH-Programme haben und SSHCraft nicht im Plan hatte,
kommen in den Plan; SFTP ebenso, aber nicht in den ersten Release. Herkunft, Programme und
Quellen jeder Zeile: [2026-11-14-sshcraft-marktvergleich.md](2026-11-14-sshcraft-marktvergleich.md).
Die Einteilung ist Claudes Vorschlag: In die erste Version kommt, was eine Schwäche anderer
Programme behebt (Abschnitt 17); Komfort kommt danach.

### 16.1 Vor der ersten Version (V1)

| Nr. | Teilplan | Inhalt |
|---|---|---|
| F1 | Verbindung und Konfiguration | ProxyCommand; Wiederverbinden mit Backoff, Tunnel neu aufbauen, Netzwechsel und Ruhezustand erkennen; `~/.ssh/config` laufend lesen und auf Wunsch zurückschreiben, mit `Include`; Hostkey-Verwaltung (ansehen, importieren, exportieren, Schlüsselwechsel des Servers über `UpdateHostKeys` annehmen); Test auf strict-kex gegen Terrapin |
| F2 | Anmeldung und Datensicherheit | Passwort als Verweis auf einen Passwortmanager (Proton Pass, 1Password, Bitwarden, KeePassXC), geholt beim Verbinden; TOTP für keyboard-interactive; App-Sperre auf dem Desktop nach Leerlauf (Windows Hello, Touch ID); Papierkorb und Versionen für Hosts, Gruppen, Schlüssel und Snippets; verschlüsselte Sicherung als Datei und Wiederherstellung; Liste aller Verbindungen, die SSHCraft selbst aufbaut |

Dazu kommen Anforderungen, die direkt in den Neuaufbau gehören (Abschnitt 17): Regex-Suche,
Ligaturen und Graphem-Cluster im Terminal (R3), Leistungsbudgets (R3), AccessKit (R4, R5), keine
Telemetrie (R2), signierte Pakete (R7, V1).

### 16.2 Nach der ersten Version

Reihenfolge als Vorschlag; Moritz kann sie umstellen.

| Nr. | Teilplan | Inhalt |
|---|---|---|
| M1 bis M3 | Mobil | früher Teilpläne 18 bis 20 (Android-Versuch, Android, iOS und iPadOS), erweitert um: Mosh und Eternal Terminal, Verbindung im Hintergrund halten (iOS: Live Activities, Standort als Opt-in), Dateien-App, Benachrichtigungen vom Server (OSC 9/777, BEL), Widgets und Kurzbefehle, Teilen-Menü zum Hochladen, Hardware-Tastatur mit eigenen Kürzeln, frei belegbare Zusatzleiste und eigene Bildschirmtastatur, Touch für tmux, YubiKey über NFC, mehrere Fenster auf dem iPad |
| S1 | SFTP | zwei Bereiche (lokal und entfernt, auch Server zu Server); Browser folgt dem Verzeichnis des Terminals, "Terminal hier öffnen"; Bearbeiten mit automatischem Hochladen; Warteschlange mit Pause und Fortsetzen nach Abbruch, mit Prüfsumme; parallele Übertragung, Bandbreitengrenze, Regeln beim Überschreiben; Rechte für viele Dateien; Vergleichen, Synchronisieren, Spiegeln als gespeicherter Auftrag; synchrones Blättern, Lesezeichen, Filter, Suche; `sudo` im Browser; SCP als Rückfall; Ziehen aus dem Dateimanager ins Terminal lädt hoch. Die Agent-Kategorie "Dateien" beginnt auf "gesperrt" (Electerm-Lücke, Marktvergleich Abschnitt 4) |
| F3 | Terminal-Komfort | Shell-Integration (OSC 133: Befehlsmarken, Exit-Code, Laufzeit, Sprung zwischen Befehlen, Ausgabe markieren, falten, Zeitstempel), Befehlsblöcke; Einspielen von terminfo und Shell-Integration beim Verbinden, nur im Speicher der Sitzung; Meldung bei Befehlsende und bei OSC 9/777; Trigger und Schlüsselwort-Hervorhebung; Farbe, Theme oder Abzeichen je Host oder Gruppe; Hinweismodus; Einfüge-Verlauf; Compose-Bereich; Snippets als Knopfleiste und mit Platzhaltern; Anmeldeskripte (Erwarten und Senden); Agent-Status in der Liste (OSC 7501) |
| F4 | Verbindungen und Ordnung | Verbindung wiederverwenden (ControlMaster); Befehle vor und nach der Verbindung (Wake-on-LAN, Port-Knocking, VPN); X11-Weiterleitung mit vorhandenem X-Server; Unix-Socket-Weiterleitung; `ssh://`-Links (öffnen nur den Verbindungsdialog); Erreichbarkeit in der Hostliste; Schnellverbinden `benutzer@host:port`; Sammelbearbeitung; Teilen mit Platzhalter für Zugangsdaten; portabler Modus; Desktop-Verknüpfung; sichere Tastatureingabe (macOS); Ressourcenleiste (Vorgabe aus) |
| F5 | Dauerhafte Sitzungen | Sitzungen, die Netzwechsel, Ruhezustand und Neustart des Clients überleben (über tmux oder screen auf dem Server, Zustand im Tab); tmux-Steuermodus (tmux-Fenster als Tabs und Bereiche); Mosh und Eternal Terminal auf dem Desktop; Telnet |
| F6 | Terminal-Extras | Bilder im Terminal (Sixel, Kitty Graphics, iTerm2-Protokoll); Hex-Ansicht für seriell; Hotkey- und Dropdown-Fenster; Dateien über das Terminal (trzsz, ZMODEM), nur nach Rückfrage, nie automatisch |
| F7 | Ziele und Automatisierung | Container als Ziel (Docker, Podman, LXD, Kubernetes, Proxmox); Weiterleitung mit Startaktion (RDP, VNC, Browser über den Tunnel); Smartcards (PIV, PKCS#11); Kerberos (GSSAPI); Makros aus dem Befehls-Journal der Engine aufnehmen und abspielen; Skripte über `sshcraft-cli run --script`; dynamische Gruppen |

Weiter vorgemerkt ohne Teilplan: mehrere Personen im selben Terminal, Import aus AWS,
DigitalOcean, Azure und Hetzner, KI-Funktionen (ab Werk aus), Plugins, Notebooks, eine
Web-Fassung mit Relay.

### 16.3 Bewusst nicht

RDP, VNC und Spice als eigene Tabs (die Startaktion in F7 reicht), ein eigener X-Server,
verschleiertes SSH, FTP-zu-SFTP-Brücke, FTPS, WebDAV und S3 im Dateibrowser, SFTP als
Windows-Laufwerk, Sync über Git, Gist oder WebDAV, FIPS-Modus, Check-out und Vier-Augen-Freigabe,
Rollen und SSO für Teams, Video-Aufzeichnung, Eingabe im Vim-Modus.

## 17. Regeln gegen die Schwächen anderer Programme

Moritz, 14.11.2026: Schwächen anderer Programme sollen in SSHCraft behoben sein. Belege stehen
im Marktvergleich, Abschnitt 4. Was dort "erfüllt" ist, hat schon Tests und zieht mit
Abschnitt 10 um. Die folgenden Regeln gelten ab dem Neuaufbau und stehen englisch in `AGENTS.md`
und `docs/security.md`; jede bekommt einen Test, wo sie sich testen lässt.

1. **Kein Shell-String aus fremden Daten.** Dateinamen, Pfade, Hostnamen und Ausgaben vom Server
   oder aus Importen werden nie in eine Befehlszeile eingesetzt, die eine Shell auswertet. Lokal
   starten Programme nur mit Argumentlisten (Lücken in Tabby und Electerm).
2. **Links öffnen nie etwas Ausführbares.** `ssh://` und andere Links öffnen höchstens den
   Verbindungsdialog mit vorbefüllten, geprüften Feldern. Es gibt keinen Link, der Befehle
   ausführt (`tabby://run`, Deep-Link-Lücke in Electerm).
3. **Escape-Folgen sind begrenzt.** OSC 52 darf nur schreiben, nie lesen, und nur nach
   Zustimmung je Host. Dateiübertragung über das Terminal (ZMODEM, trzsz) startet nie von selbst.
   Abfragen, deren Antwort Eingaben einschleusen kann (etwa die Titel-Abfrage), sind aus.
4. **Pfade aus dem Netz werden normalisiert und begrenzt**, bevor sie lokal etwas berühren
   (Pfad-Traversal im SFTP von Tabby). Das gilt ab S1 und schon für Protokoll-Export und Importe.
5. **Nichts dauerhaft auf dem Zielsystem ablegen**, ohne dass der Benutzer es ausdrücklich will
   (iTerm2, CVE-2025-22275).
6. **Keine Telemetrie.** Jede Verbindung, die SSHCraft selbst aufbaut (Sync-Server,
   Update-Prüfung, Passwortmanager), steht in einer Liste in den Einstellungen. Die
   Update-Prüfung läuft nur nach Zustimmung.
7. **Krypto nur aus geprüften Bibliotheken**, Signaturen mit deterministischen Nonces
   (RFC 6979). strict-kex gegen Terrapin hat einen Test (PuTTY CVE-2024-31497, CVE-2023-48795).
8. **Agent-Schnittstellen ohne HTTP**, Token an jeder Verbindung, Rechte je Aktion
   (MCP-Lücken in Electerm).
9. **Leistungsbudgets als Tests:** 1 GB Ausgabe über `cat` lässt die Oberfläche nicht hängen;
   ein Speicherbudget je Sitzung und eines für die leere App.
10. **Lokale Daten bleiben maßgeblich.** Der Sync löscht lokal nie ohne Grabstein und
    Papierkorb; es gibt eine verschlüsselte Sicherung als Datei (leerer Tresor bei Termius).
11. **Barrierefreiheit:** egui mit AccessKit, das Terminal ist für Screenreader als linearer Text
    lesbar.
12. **Lieferkette:** eine offizielle Download-Adresse, signierte Pakete über winget, Homebrew und
    Flathub, Prüfsummen, SBOM über `supply-chain.yml`; reproduzierbare Builds als Ziel
    (Hintertür in Xshell, gefälschte PuTTY-Downloads).
13. **Jede Sicherheitslücke bekommt einen Test, der vorher fehlschlug** (unvollständiger Fix in
    Tabby); das verlangen die House rules ohnehin.
