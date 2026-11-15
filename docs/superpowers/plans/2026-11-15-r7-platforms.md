# SSHCraft Teilplan R7: Plattformen und Pakete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SSHCraft baut, testet und packt auf Windows, macOS, Linux und FreeBSD wie die Craft-Apps: integrierte Titelleiste mit Menüleiste, Schriften aus craft-fonts, Pakete (MSI, DMG, deb, rpm, AppImage, Flatpak, FreeBSD-Archiv), die CI-Dateien der Familie und `docs/releasing.md`; `docs/superpowers/` verlässt den Baum.

**Architecture:**
- **Code zuerst portabel.** Alles Windows-Eigene steht schon hinter `cfg(windows)` (Windows Hello, Sicherheitsschlüssel über `webauthn.dll`, PuTTY-Registry, Pipe der Tauri-Fassung). R7 prüft das in einem Linux-Container (Docker) und in der CI auf macOS und FreeBSD und füllt die Lücken: Schlüssel im Schlüsselbund über `keyring` (Keychain, Secret Service), Sicherheitsschlüssel und Windows Hello gibt es außerhalb von Windows nicht (die Seite sagt es).
- **Fenster wie in der Familie.** `craft-window` entscheidet über die Titelleiste (integriert, oder die des Systems auf Wunsch und auf kachelnden Fenstermanagern), `craft-menu` baut aus den Befehlen der Engine und der Oberfläche eine Menüleiste (unter macOS die native), `craft-quit` fragt vor dem Beenden bei laufenden Sitzungen.
- **Pakete sind Kopien der Skripte von gridcraft**, umbenannt auf SSHCraft (App-Id `de.musiker15.sshcraft`, Programme `sshcraft` und `sshcraft-cli`), mit eigenem Upgrade-Code und eigenem Icon. Signieren und Veröffentlichen brauchen Geheimnisse und eine `release`-Umgebung, die nur Moritz einrichtet: R7 legt die Dateien an, löst nie einen Release aus.

**Tech Stack:** wie R6. Neu: `craft-window` (Feature `egui`), `craft-menu`, `craft-quit` (Feature `egui`), `craft-fonts-build` (und als Build-Abhängigkeit), alle aus craft-libs `cebc847`; `winresource` für Icon und Versionsinfo unter Windows; Werkzeuge der Pakete (WiX 5.0.2, nfpm 2.47.0, appimagetool, flatpak-builder) nur in der CI.

**Spec:** `docs/superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`, Abschnitte 7.1, 11, 12, 13 (Zeile R7), 16.1. Vorlage: `storytold/gridcraft` (`packaging/`, `.github/workflows/`, `docs/releasing.md`), `storytold/craft-libs` (`crates/{window,menu,quit,fonts-build}`), `storytold/pdfcraft/crates/ui-egui/src/menus.rs`. Offene Punkte: `docs/gaps.md` 1a (Titelleiste, Menü, Schriften) und 3 (nur Windows).

## Global Constraints

- **Arbeitsbaum** `G:\Claude\worktrees\sshcraft-r7`, Zweig `r7-platforms`, von `r6-control`; gemergte PRs #12 und #13 kommen über `origin/main` herein.
- **Sprache:** Code, Kommentare, Doku, Commits englisch, Aufgaben-Id je Commit (`R7.3: windows package`). Kein KI-Hinweis. Plan deutsch.
- **Kein Release, kein Tag, kein Push auf einen `release`-Zweig, keine Geheimnisse.** `release.yml` läuft nur per Hand oder auf `release`; Moritz richtet Zweig, Umgebung und Geheimnisse ein (`docs/releasing.md` sagt wie).
- **App-Kennung bleibt `de.musiker15.sshcraft`** (Keyring-Dienst, Ordner, Flatpak-Id, Bundle-Id), Datenordner und Schema 11 bleiben.
- **Kein Asset eines anderen Herstellers.** Das App-Icon ist eigene Arbeit (das grüne Terminal-Zeichen der App), jede Datei mit Zeile in `ATTRIBUTION.md`; Schriftdateien nie im Repository.
- **CI kostet Minuten (privates Repository, macOS zählt zehnfach):** der Gate läuft auf Ubuntu und Windows bei jedem PR, macOS auf `main` und für PRs mit dem Label `full-ci`; FreeBSD und die Pakete bei Änderungen an ihren Dateien und per Hand.
- **Nie abstürzen, kein `unsafe`** außerhalb von `crates/fido` (unverändert).
- **Jede Aufgabe endet mit den betroffenen Paketen grün** (clippy, Tests, layers, assets), Linux im Container, der volle `cargo xtask ci` vor dem PR.

## Review Focus

1. **Auf Linux ohne Secret Service (kein laufender Schlüsselbund)**: Erwartet: der Tresor sagt klar, dass er keinen Schlüsselbund findet, die App startet und zeigt es, nichts stürzt ab und kein neuer Hauptschlüssel überschreibt einen alten (Test `a_missing_keyring_is_reported_not_replaced`, Task 1).
2. **Ein Klick auf ein Menü, während eine Rückfrage offen ist**: Erwartet: das Menü tut nichts, die Rückfrage bleibt (Test `the_menu_is_closed_while_a_question_is_open`, Task 4).
3. **Beenden mit laufenden Sitzungen**: Erwartet: eine Frage, ob die Sitzungen enden sollen; Abbrechen lässt alles laufen (Test `quitting_with_sessions_asks_first`, Task 4).
4. **Ein Build ohne `CRAFT_FONTS_DIR`**: Erwartet: die App baut und läuft mit ihren eingebauten und den System-Schriften; mit `CRAFT_FONTS_REQUIRED=1` ist es ein Fehler (Test `fonts_are_optional`, Task 5).
5. **Das MSI ersetzt eine ältere Fassung**: Erwartet: gleicher Upgrade-Code, Installation pro Benutzer oder pro Rechner, kein Desktop-Symbol ohne Haken (Prüfung `test-install-policy.ps1` in `packaging-lint.yml`, Task 6).

---

### Task 1: Linux, macOS und FreeBSD bauen und testen

**Files:**
- Modify: was der Container und die CI finden (`crates/local/src/pty.rs`, `crates/fido`, `crates/platform`, `apps/sshcraft/src/platform.rs`, `crates/store/src/vault.rs`, die `cfg`-Stellen der Engine)
- Create: `xtask/src/linux.rs` (`cargo xtask linux`: der Gate im Container `rust:<version>` mit den Paketen der Familie, Zielordner im Container)
- Test: `crates/store/src/vault_tests.rs`, `crates/ui-egui/tests/keys.rs`

- [ ] **Step 1: Tests:** `a_missing_keyring_is_reported_not_replaced` (ein Schlüsselbund, der nicht antwortet: Fehler mit Grund, kein neuer Hauptschlüssel), `security_keys_say_where_they_work` (ohne Windows: die Schlüsselseite bietet keinen Sicherheitsschlüssel an und sagt warum).
- [ ] **Step 2:** `cargo xtask linux` laufen lassen; jede Fehlstelle beheben, ohne Windows zu ändern. **Step 3:** grün im Container. Commit `R7.1: linux, macos and freebsd builds`.

### Task 2: CI-Dateien der Familie

**Files:**
- Modify: `.github/workflows/ci.yml` (Gate auf `["ubuntu-24.04","windows-latest"]`, macOS auf `main` und mit Label `full-ci`; `packages` wie gridcraft)
- Create: `.github/workflows/{freebsd.yml,windows-arm64.yml,packaging-lint.yml,release.yml}` (Aufrufe der geteilten Workflows; `release.yml` nur `workflow_dispatch` und Push auf `release`, ohne nächtlichen Lauf)

- [ ] **Step 1:** Dateien; `actionlint` im Container. **Step 2:** Push, CI auf allen drei Systemen grün. Commit `R7.2: ci for every platform`.

### Task 3: App-Icon und Versionsinfo

**Files:**
- Create: `assets/app-icon/sshcraft.svg` (eigene Zeichnung), `packaging/icons.sh`, die erzeugten PNG/ICO/ICNS, `apps/sshcraft/build.rs` (winresource: Icon und VERSIONINFO), `xtask/src/ico.rs` (`cargo xtask ico`)
- Modify: `ATTRIBUTION.md`, `crates/ui-egui/src/widgets/icons.rs` (das Fenster-Icon aus dem PNG)

- [ ] **Step 1: Test** `the_window_icon_is_the_app_icon` (Größe, Transparenz der Ecken), `cargo xtask assets` grün mit den neuen Dateien. Commit `R7.3: app icon`.

### Task 4: Integrierte Titelleiste, Menüleiste, Beenden

**Files:**
- Modify: `apps/sshcraft/src/main.rs` (`craft_window::policy::decide_for(Prefix::new("SSHCRAFT"), …)`, `apply`), `crates/ui-egui/src/shell.rs` (Titelleiste mit Fensterknöpfen bei `Frame::Integrated`, `title_bar::empty_area`), Einstellungen › Allgemein ("Use system title bar")
- Create: `crates/ui-egui/src/menus.rs` (Menüleiste aus `ui_commands()` und den Engine-Befehlen mit Menüpfad; `craft_menu::layout`, unter macOS `craft_menu::mac::install_with`), Quit über `craft-quit` mit Frage bei laufenden Sitzungen
- Test: `crates/ui-egui/tests/menus.rs`

- [ ] **Step 1: Tests:** `the_menu_runs_its_command` (Datei › Neuer Tab öffnet einen Tab), `the_menu_is_closed_while_a_question_is_open`, `quitting_with_sessions_asks_first`, `the_system_title_bar_is_a_setting`, `menu_items_show_their_shortcuts`.
- [ ] **Steps 2–4**; Gegenprobe: Menü bei offener Rückfrage nicht gesperrt → rot. Commit `R7.4: title bar, menus and quit`.

### Task 5: Schriften aus craft-fonts

**Files:**
- Create: `crates/ui-egui/build.rs` (`craft_fonts_build::Embed`), Modify: `crates/ui-egui/src/theme/` (eingebettete Schriften vor den System-Schriften), `crates/ui-egui/Cargo.toml`
- Test: `crates/ui-egui/tests/fonts.rs`

- [ ] **Step 1: Test** `fonts_are_optional` (ohne `CRAFT_FONTS_DIR` baut und rendert es; die Tabelle ist leer), CI mit `craft-fonts-ref` der Familie. Commit `R7.5: fonts from craft-fonts`.

### Task 6: Windows-Paket

**Files:**
- Create: `packaging/env.sh`, `packaging/windows/{sshcraft.wxs,install-policy.json,install-policy.wxi,package.ps1,sign.ps1,check-install-policy.ps1,test-install-policy.ps1,INSTALLATION.md}`
- [ ] MSI pro Benutzer oder pro Rechner (`%LOCALAPPDATA%\Programs\SSHCraft` bzw. `Program Files\SSHCraft`), `sshcraft.exe` und `sshcraft-cli.exe` (CLI im `PATH`), eigener Upgrade-Code, Desktop-Symbol nur mit Haken, `ssh://`-Links nicht registriert (Spec 17: Links öffnen höchstens den Dialog; kommt mit F1). Prüfung in `packaging-lint.yml`. Commit `R7.6: windows package`.

### Task 7: macOS-, Linux- und FreeBSD-Pakete

**Files:**
- Create: `packaging/macos/{Info.plist.in,entitlements.plist,import-cert.sh,package.sh}`, `packaging/linux/{de.musiker15.sshcraft.desktop,de.musiker15.sshcraft.metainfo.xml.in,nfpm.yaml,package.sh,postinst.sh,flatpak/de.musiker15.sshcraft.yml,flatpak/de.musiker15.sshcraft.bundle.yml,flatpak-bundle.sh}`, `packaging/freebsd/package.sh`
- [ ] Skripte wie gridcraft; im Container `packaging/linux/package.sh` für deb, rpm, tar.gz laufen lassen (AppImage und Flatpak nur in der CI). Flatpak: `--share=network` (SSH braucht es), `--talk-name=org.freedesktop.secrets`. Commit `R7.7: macos, linux and freebsd packages`.

### Task 8: Doku und Abschluss

**Files:**
- Create: `docs/releasing.md` (Zweig `release`, Umgebung, Geheimnisse, Entwurf, Prüfen, Veröffentlichen; was Moritz einmal einrichtet)
- Modify: alle Verweise auf `docs/superpowers/` in `AGENTS.md`, `docs/architecture.md`, `docs/security.md` und den Paritätsseiten zeigen auf englische Doku (`docs/decisions.md`, `docs/architecture.md`); dann `git rm -r docs/superpowers` (die Geschichte behält es); `ROADMAP.md` (R7 done), `docs/roadmap.md`, `docs/gaps.md`, README
- [ ] Abschluss-Review (opus), alle Befunde beheben, auch die kleinen, jeder mit Test, der vorher rot war. `cargo xtask ci`, PR mit Auto-Merge (Merge-Commit), Liste für die Probe von Hand. Commit `R7.8: docs`.
