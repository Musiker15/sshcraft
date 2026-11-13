# SSHCraft Teilplan 15: Agent-Anbindung Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** KI-Agents (Claude Code, Codex und andere) arbeiten über einen MCP-Server `sshcraft mcp` und eine Kommandozeile `sshcraft` mit dem laufenden Desktop-Client: Hosts, Gruppen und Schlüsselnamen ansehen, Befehle ausführen, Sitzungen führen, Hosts und Gruppen pflegen, Schlüssel erzeugen und importieren. Nie sehen sie privates Schlüsselmaterial. Jede Aktion hat eine Rechtestufe, Rückfragen beantwortet nur die Oberfläche von SSHCraft, Hosts und Gruppen lassen sich ausblenden, alles landet in einem Protokoll.

**Architecture:**
- **Vermittler:** Ein neues Programm `sshcraft` (Crate `sshcraft-cli`) spricht über eine lokale Verbindung mit dem laufenden Client: unter Windows eine Named Pipe, die nur der eigene Benutzer öffnen kann, unter Unix ein Socket mit Rechten `0600` in einem Ordner `0700`. `sshcraft mcp` ist ein MCP-Server über stdio (JSON-RPC 2.0, von Hand, ohne SDK), die übrigen Unterbefehle sind dieselben Aktionen für Skripte.
- **Protokoll** (Crate `sshcraft-agent`, von Client und CLI geteilt): je Zeile ein JSON-Objekt, Anfrage `{ id, agent, action }`, Antwort `{ id, ok | error }`.
- **Im Client** (Modul `app/src-tauri/src/agents/`): Server auf der Verbindung, Prüfung (ausgeblendet, Rechtestufe, Rückfrage), Protokolleintrag, dann die Aktion über eine Schnittstelle `AgentBackend`, deren Tauri-Fassung die vorhandenen Befehle und den `SessionManager` nutzt.
- **Kern** (`crates/sshcraft-core/src/agent.rs`): Rechte, Ausblenden, Protokoll (Schema 11, versiegelt).

**Tech Stack:** wie Teilplan 14; neu `windows`-Features für Pipe-Sicherheit und Prozessbesitzer, `clap` (schon im Workspace) für die CLI.

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`, Abschnitt 10 ("Anbindung für KI-Agents") vollständig, Abschnitt 11.

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 14 gelten weiter (Arbeitsbaum `G:\Claude\worktrees\sshcraft-t15`, Zweig `teilplan-15-agents`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung, Gegenproben für Regeln, Rechte und Datenwege, lokal mergen, Texte über i18n).
- **Nie Schlüsselmaterial:** Keine Aktion liefert private Schlüssel, Passphrasen, Passwörter, Konto- oder Geräteschlüssel. Schlüssel erscheinen nur als `{ id, name, type, fingerprint }`, Hosts ohne Passwort-Ids. Ein Test sucht in jeder Antwort nach `PRIVATE KEY`.
- **Aktionen** (`Action`, snake_case) und ihre **Kategorie**:
  - `list` (Erlaubt): `list_hosts`, `list_groups`, `list_keys`, `host_info { host }`.
  - `connect` (Fragen): `exec { host, command, timeout_secs? }` (Ausgabe höchstens 1 MiB, Zeit 1 bis 600 s, Vorgabe 60), `session_open { host }`, `session_write { session, data }`, `session_read { session, wait_ms? }` (höchstens 1 MiB je Lesen, Warten höchstens 10 s), `session_close { session }`.
  - `edit` (Fragen): `save_host { host }`, `save_group { group }`.
  - `delete` (Gesperrt): `delete_host { host }`, `delete_group { group }`, `delete_key { key }`.
  - `keys` (Fragen): `generate_key { name, type }` (ed25519, ecdsa, rsa), `import_key { name, text, passphrase? }`.
  - `share` und `sync` (Gesperrt): in den Einstellungen sichtbar, ohne Aktionen in dieser Fassung.
  - Einstellungen und Agent-Rechte: keine Aktion, auch nicht lesend.
- **Rechte:** `AgentLevel = allow | ask | deny` je Kategorie, Einstellung `agents.policy` lokal auf dem Gerät. Fehlt eine Kategorie, gilt ihre Vorgabe.
- **Ausblenden:** eine Liste von Host- und Gruppen-Ids. Ein ausgeblendeter Host und jeder Host unter einer ausgeblendeten Gruppe (auch tiefer) erscheint in keiner Liste, und jede Aktion auf ihn antwortet wie auf einen unbekannten Host (`not_found`). Eine neue Gruppe oder ein neuer Host von einem Agent darf nicht unter eine ausgeblendete Gruppe.
- **Rückfrage:** ein Fenster in SSHCraft mit Agent, Aktion, Ziel (Hostname, Adresse) und bei Befehlen dem Befehl; Erlauben oder Ablehnen; nach 120 s gilt Ablehnen. Der Agent erfährt nur `denied`.
- **Protokoll:** je Aktion ein Eintrag `{ at, agent, action, target, detail, outcome: allowed | asked_allowed | asked_denied | denied | hidden | failed }`, `detail` bei Befehlen der Befehl (höchstens 2000 Zeichen). Versiegelt je Eintrag (Label `agent-log:<id>`), höchstens 1000 Einträge, älteste gehen zuerst. Ansicht und Löschen in den Einstellungen.
- **Verbindung:**
  - Windows: `\\.\pipe\sshcraft-agent-<SID des Benutzers>`, DACL nur für diese SID, `reject_remote_clients`, erste Instanz (`first_pipe_instance`). Die CLI prüft nach dem Verbinden, dass der Server-Prozess demselben Benutzer gehört (`GetNamedPipeServerProcessId`, Token-SID), sonst bricht sie ab.
  - Unix: `$XDG_RUNTIME_DIR/sshcraft/agent.sock`, sonst `~/.sshcraft/agent.sock`.
  - Eine Zeile höchstens 4 MiB; Agent-Name 1 bis 64 druckbare Zeichen.
  - Läuft der Client nicht: Meldung "SSHCraft läuft nicht" mit Exit-Code 3 (CLI) bzw. Tool-Fehler (MCP).
- **MCP:** Protokollversion `2025-06-18`, `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`; der Agent-Name ist `clientInfo.name`. Jede Aktion ein Tool `sshcraft_<action>` mit JSON-Schema.
- **CLI:** `sshcraft hosts|groups|keys`, `sshcraft exec <host> -- <befehl…>`, `sshcraft session open|write|read|close`, `sshcraft host save <json>`, `sshcraft mcp`, Option `--agent <name>` (Vorgabe `cli`), Ausgabe JSON mit `--json`, sonst lesbar. `<host>` ist Id oder eindeutiger Name.
- Nur Desktop.

## Review Focus

1. Ein anderer Windows-Benutzer auf demselben Rechner legt die Pipe zuerst an oder verbindet sich: er bekommt keinen Zugriff, die CLI redet nicht mit ihm.
2. Ein Agent versucht über Umwege an Geheimes zu kommen (Host mit Passwort-Id, `host_info`, Fehlermeldungen mit Schlüsseltext, Import-Antwort): nichts davon enthält Material.
3. Ein ausgeblendeter Host wird über Name, Id, Sprung-Host-Liste oder eine Gruppe erreicht: nichts verrät ihn.
4. Viele Rückfragen auf einmal oder ein Agent, der während der Rückfrage die Verbindung schließt: kein hängender Dialog, keine Ausführung ohne Ja.
5. Eine laufende Agent-Sitzung mit viel Ausgabe ohne Lesen: der Client hält höchstens 1 MiB vor und bremst die Sitzung, statt Speicher zu füllen.

---

### Task 1: Kern, Rechte, Ausblenden, Protokoll

**Files:** Create `crates/sshcraft-core/src/agent.rs`, `agent_tests.rs`; Modify `lib.rs`, `store.rs` (`SCHEMA_V11`, `SCHEMA_VERSION = 11`).

**Interfaces — Produces:** `AgentCategory`, `AgentLevel`, `AgentPolicy { levels: BTreeMap<AgentCategory, AgentLevel>, hidden: BTreeSet<Uuid> }` mit `level(cat)`, `default_level(cat)`, `load(store)`, `save(store, &policy)`; `hidden_hosts(policy, hosts, groups) -> HashSet<HostId>`; `group_hidden(policy, groups, group) -> bool`; `AgentLogEntry`, `AgentOutcome`, `log_append(vault, store, entry)`, `log_list(vault, store) -> Vec<AgentLogEntry>` (neueste zuerst), `log_clear(store)`.

- [ ] Tests zuerst: Vorgaben je Kategorie wie Spec; gespeicherte Stufe gewinnt; unbekannte Kategorie im JSON wird übergangen; Host unter ausgeblendeter Untergruppe ist ausgeblendet (Gegenprobe: nur direkte Gruppe prüfen); Zyklus in Gruppen hängt nicht; Protokoll hält 1000, ältester fällt; Eintrag ist versiegelt (kein Befehlstext im Klartext der Datenbank).
- [ ] Commit "Kern: Rechte, Ausblenden und Protokoll für Agents".

### Task 2: Protokoll und Verbindung (`crates/sshcraft-agent`)

**Files:** Create crate mit `lib.rs` (`Request`, `Response`, `Action`, `AgentError { code, message }`, `HostView`, `KeyView`, `ExecResult`), `framing.rs` (Zeilen lesen und schreiben, 4 MiB), `transport/{mod.rs, windows.rs, unix.rs}` (`listen() -> Listener`, `connect() -> Stream`, `endpoint_name()`).

- [ ] Tests zuerst: Rundweg jeder Aktion durch JSON; Zeile über 4 MiB → Fehler ohne Speicherlauf; Agent-Name geprüft; Windows: Server und Client im selben Test über eine Pipe mit Testnamen, zweiter Server mit demselben Namen scheitert (`first_pipe_instance`), Besitzerprüfung des Servers gelingt für den eigenen Prozess.
- [ ] Commit "Protokoll und lokale Verbindung für Agents".

### Task 3: Prüfung und Ausführung im Client

**Files:** Create `app/src-tauri/src/agents/{mod.rs, gate.rs, gate_tests.rs, backend.rs, server.rs}`; Modify `state.rs`, `lib.rs`, `prompts.rs` (Rückfrage `agent-confirm`, Befehl `answer_agent`).

**Interfaces — Produces:** `trait AgentBackend` (eine async-Methode je Aktion), `trait Confirmer { async fn confirm(&self, prompt: AgentPrompt) -> bool }`, `Gate::handle(&self, request) -> Response` (ausgeblendet → `not_found`, Stufe, Rückfrage, Protokoll, Aufruf), `TauriBackend`.

- [ ] Tests zuerst (Fake-Backend, Fake-Confirmer): `allow` ruft ohne Rückfrage; `ask` ruft nur nach Ja, sonst `denied` und Backend unberührt (Gegenprobe); `deny` nie; ausgeblendeter Host → `not_found` auch bei `exec`, `host_info`, `delete_host` und in `list_hosts`; Speichern eines Hosts in eine ausgeblendete Gruppe → `not_found`; jede Antwort ohne `PRIVATE KEY` und ohne Passwort-Ids; Protokolleintrag je Ausgang; Rückfrage läuft nach 120 s ab (pausierte Zeit).
- [ ] Commit "Agents: Prüfung, Rückfrage, Protokoll und Server im Client".

### Task 4: Befehle und Sitzungen für Agents

**Files:** Modify `app/src-tauri/src/sessions.rs` (`exec(host, command, limit, timeout)`, `AgentSink` mit Puffer bis 1 MiB, `read_agent(session, wait)`), Tests in `sessions_tests.rs`.

- [ ] Tests zuerst gegen den Testserver: `exec` liefert die Ausgabe und hält die Grenze; Zeitüberschreitung → Fehler; Sitzung: schreiben, lesen, schließen; ohne Lesen hält der Puffer höchstens 1 MiB und die Sitzung wartet (Gegenprobe: ohne Grenze wächst er).
- [ ] Commit "Agents: Befehle und Sitzungen".

### Task 5: Kommandozeile und MCP (`crates/sshcraft-cli`, Programm `sshcraft`)

**Files:** Create crate mit `main.rs`, `cli.rs`, `mcp.rs`, `mcp_tests.rs`.

- [ ] Tests zuerst: MCP `initialize` → Fähigkeiten und Protokollversion; `tools/list` nennt jede Aktion mit Schema; `tools/call` mit falschen Argumenten → Tool-Fehler, nicht Absturz; Antwort des Clients wird Text-Inhalt; unbekannte Methode → JSON-RPC-Fehler `-32601`; CLI: `exec web -- ls -la` ergibt `exec { host: "web", command: "ls -la" }`; Client nicht erreichbar → Exit-Code 3.
- [ ] Commit "Kommandozeile sshcraft und MCP-Server".

### Task 6: Oberfläche

**Files:** Create `app/src/components/AgentConfirmDialog.tsx`, `app/src/pages/AgentSettings.tsx` (Rechte je Kategorie, ausgeblendete Hosts und Gruppen, Protokoll mit Löschen), Tests; Modify Hosts-Kontextmenü ("Für Agents ausblenden"), `AppShell.tsx`, `api.ts`, i18n.

- [ ] Tests zuerst: Dialog zeigt Agent, Aktion, Ziel und Befehl und antwortet; zwei Rückfragen nacheinander; Rechte speichern; Ausblenden über das Kontextmenü; Protokoll-Ansicht.
- [ ] Commit "Oberfläche: Rechte, Rückfrage, Ausblenden, Protokoll".

### Task 7: Auslieferung und Doku

**Files:** `app/src-tauri/tauri.conf.json` (`bundle.externalBin: ["binaries/sshcraft"]`), Skript `app/scripts/build-cli.mjs` (baut `sshcraft-cli` im Release und legt es als `binaries/sshcraft-<triple>.exe` ab, in `beforeBuildCommand`), `docs/agents.md` (Einrichtung für Claude Code und Codex, Rechte, Grenze `pass-cli`), `docs/security.md`, `README.md`, Fahrplan Zeile 15.

- [ ] Prüfen: Installer enthält `sshcraft.exe`; alle Tests, clippy, fmt; Commit "Auslieferung der Kommandozeile, Doku".
