# SSHCraft: Fahrplan der Teilpläne

Design: `docs/superpowers/specs/2026-11-11-sshcraft-design.md` (freigegeben am 11.11.2026, Umfang
erweitert am 12.11.2026 auf den Funktionsumfang von Termius ohne SFTP, dazu Englisch und Deutsch).

**Seit 14.11.2026 gilt der Neuaufbau nach den Konventionen der Craft-Apps**
(`docs/superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`): Die Teilpläne 1 bis 15
sind in der Tauri-Fassung fertig (Tag `tauri-final`); ihr Funktionsumfang wird in den Teilplänen
R1 bis R7 in egui neu aufgebaut. Die Teilpläne 16 bis 20 gehen in R7, V1 und M1 bis M3 auf. Neue
Funktionen aus dem Marktvergleich (`docs/superpowers/specs/2026-11-14-sshcraft-marktvergleich.md`)
und SFTP stehen in der zweiten Tabelle.

Das Design umfasst mehrere voneinander trennbare Teilsysteme. Jeder Teilplan unten ergibt für
sich lauffähige, getestete Software. Ausführlich geschrieben wird immer nur der nächste Teilplan,
mit dem Wissen aus den vorherigen.

| Nr. | Teilplan | Ergebnis | Stand |
|---|---|---|---|
| 1 | Desktop-Grundlage | Windows-App: Gruppen und Hosts, eigene Schlüssel (verschlüsselt gespeichert), Terminal in Tabs, Host-Fingerprint-Prüfung, Keep-Alive, Wiederverbinden | fertig, auf `main` |
| 2 | Schlüsselquellen und Windows-Anbindung | Proton-Pass-Agent, Import über `pass-cli`, Import aus `~/.ssh/config`, Windows Hello | fertig, auf `main` |
| 3 | Protokoll- und Krypto-Crate | `sshcraft-proto`: Konto- und Geräteschlüssel, Geräteprotokoll, Bereichsschlüssel, signierte und verschlüsselte Datensätze, Rückfallschutz | fertig, auf `main` |
| 4 | Oberfläche nach Termius-Vorbild und Sprachen | neuer Aufbau mit Seitenleiste (Hosts, Schlüssel, Snippets, Portweiterleitung, Protokolle, Einstellungen), Befehlspalette, Neuer-Tab-Seite, Themes und Schriften, Übersetzung Englisch und Deutsch mit AUTO; Fehlercodes aus Rust statt Texten | fertig, auf `main` |
| 5 | Sync-Server | `sshcraft-server`: axum, SQLite und PostgreSQL, API-Key, Challenge-Anmeldung, Konten, Geräteprotokolle, Datensätze, Push per WebSocket, Verwaltungsbefehle, Docker-Image, Beispiele für Apache und nginx | fertig, auf `main` |
| 6 | Client-Sync | Mehrere Server, Konto anlegen, Gerät einrichten (Einfügen, QR), Sync-Einstellungen, Live-Abgleich, Offline-Betrieb, Geräteliste und Geräte-Sperre mit Schlüsseltausch | fertig, auf `main` |
| 7 | Teilen | Freigaben für Host und Gruppe, Auswahl, mit oder ohne Schlüssel, lesen oder schreiben, Sicherheitsnummer, Entziehen mit Schlüsseltausch | fertig, auf `main` |
| 8 | Hosts und Ordnung | Untergruppen, Tags, Host-Symbole, Import aus `known_hosts`, Schlüsseldateien, CSV, PuTTY, MobaXterm, SecureCRT, Export als CSV und JSON | fertig, auf `main` |
| 9 | Verbindungen | Passwort und keyboard-interactive, Identitäten, Sprung-Hosts, HTTP- und SOCKS5-Proxy, Agent-Weiterleitung, Umgebungsvariablen, serielle Verbindung, lokales Terminal | fertig, auf `main` |
| 10 | Schlüssel erweitert | ECDSA und RSA erzeugen, Passphrase speichern, öffentlichen Schlüssel in `authorized_keys` eintragen, SSH-Zertifikate, FIDO2-Hardwareschlüssel, gerätegebundene Schlüssel (Windows Hello mit TPM) | fertig, auf `main` |
| 11 | Workspaces | Split und Fokus mit 2 bis 16 Sitzungen, Broadcast, Workspace-Vorlagen, Statuspunkte, Tastenkürzel, Terminaltyp, OSC 52, Links | fertig, auf `main` |
| 12 | Portweiterleitung | lokal, entfernt, dynamisch (SOCKS), Regelseite, Assistent | fertig, auf `main` |
| 13 | Snippets und Autovervollständigung | Snippets in Paketen, auf mehreren Hosts ausführen, Seitenleiste mit Verlauf, Autovervollständigung aus Verlauf, Pfaden, Snippets und Befehlen (ohne KI) | fertig, auf `main` |
| 14 | Sitzungsprotokolle | Aufzeichnung, Lesezeichen, Kommentare, Suche, Löschen, Export, Aufbewahrung, Sync | fertig, auf `main` |
| 15 | Agent-Anbindung | `sshcraft mcp`, Kommandozeile `sshcraft`, Rechte pro Aktion, Bestätigungsfenster, Ausblenden, Protokoll | fertig, auf `main` |
| 16 | Desktop-Plattformen | macOS, Linux und FreeBSD: Agent über `SSH_AUTH_SOCK`, Keychain und Secret Service, Touch ID, Login-Shell als lokales Terminal, `pass-cli`-Pfade; CI-Matrix für alle Desktop-Ziele | geht in R7 auf |
| 17 | Releases für Desktop und Server | GitHub-Actions-Releases: Windows x64, x86, arm64; macOS universal; Linux x86_64 und aarch64; FreeBSD x86_64; Server-Programm für Linux; Docker-Image; Windows-Pakete bei winget eingetragen und bei jedem neuen Release per Workflow automatisch auf winget aktualisiert | geht in V1 auf |
| 18 | Android-Spike (Wegwerf) | Urteil zur Handytastatur mit xterm.js im Webview | überholt (kein Webview mehr), neu als M1 mit egui; Zweig `spike/android` bleibt liegen |
| 19 | Android vollständig | Keystore, Zusatzleiste, Gesten, Lautstärketasten, Vordergrund-Dienst, App-Sperre, gerätegebundene Schlüssel, APK in den Releases | geht in M2 auf |
| 20 | iOS und iPadOS | Keychain, Face ID, Secure Enclave, Wiederverbinden beim Zurückkehren, iPad-Layout mit Seitenleiste, Hardwaretastatur und Split View, IPA-Bau auf macOS-Runner, Anleitung zum Sideloading | geht in M3 auf |

### Neuaufbau und danach (ab 14.11.2026)

Einzelheiten je Zeile: Neuaufbau-Entwurf, Abschnitte 13 und 16.

| Nr. | Teilplan | Ergebnis | Stand |
|---|---|---|---|
| R1 | Gerüst und Regeln | Craft-Workspace, Lizenz MIT OR Apache-2.0, `AGENTS.md` mit House rules, xtask, Lints; Tauri und `sshcraft-agent` entfernt; Kern-Crates umgezogen | offen |
| R2 | Engine | `Session`, Befehlsregister, Agent-Prüfung, Absturzschutz, keine Telemetrie | offen |
| R3 | Terminal | `term` auf `alacritty_terminal`, egui-Renderer, Regex-Suche, Ligaturen, Leistungsbudgets | offen |
| R4 | Oberfläche I | Fenster, Seitenleiste, Tabs, Befehlspalette, Tokens, Sprachen, Hosts, Schlüssel, Verbinden, AccessKit | offen |
| R5 | Oberfläche II | Workspaces, Weiterleitung, Snippets, Protokolle, Sync, Teilen, Agent-Rückfrage | offen |
| R6 | Steuerung | Control-Channel, MCP-Crate, `sshcraft-cli`, Launcher-Anbindung | offen |
| R7 | Plattformen und Pakete | macOS, Linux, FreeBSD, `packaging/`, CI über craft-common-actions | offen |
| F1 | Verbindung und Konfiguration | ProxyCommand, Wiederverbinden mit Backoff, `~/.ssh/config` laufend, Hostkey-Verwaltung, strict-kex-Test | offen |
| F2 | Anmeldung und Datensicherheit | Passwort-Verweise auf Passwortmanager, TOTP, Desktop-App-Sperre, Papierkorb, verschlüsselte Sicherung, Liste der eigenen Verbindungen | offen |
| V1 | Erste Version | signierte Pakete für alle Desktop-Ziele, Prüfsummen, SBOM, winget, Homebrew, Flathub, Server-Programm, Docker-Image | offen |
| M1 bis M3 | Mobil | Android-Versuch mit egui, Android, iOS und iPadOS, erweitert um Mosh, Eternal Terminal, Dateien-App, Benachrichtigungen, Widgets, NFC-YubiKey | offen |
| S1 | SFTP | Dateibrowser mit zwei Bereichen, folgt dem Terminal, Bearbeiten mit Hochladen, Warteschlange mit Fortsetzen, Synchronisieren | offen |
| F3 | Terminal-Komfort | Shell-Integration (OSC 133), Meldungen, Trigger, Farbe je Host, Compose-Bereich, Anmeldeskripte | offen |
| F4 | Verbindungen und Ordnung | ControlMaster, Befehle vor und nach der Verbindung, X11, Schnellverbinden, Sammelbearbeitung, portabler Modus | offen |
| F5 | Dauerhafte Sitzungen | tmux-Integration, Sitzungen über Neustart hinweg, Mosh, Eternal Terminal, Telnet | offen |
| F6 | Terminal-Extras | Bilder im Terminal, Hex-Ansicht, Dropdown-Fenster, trzsz und ZMODEM nur nach Rückfrage | offen |
| F7 | Ziele und Automatisierung | Container, Startaktion über Tunnel (auch in den Datenbank-Client), Smartcards, Kerberos, Makros, Skripte, dynamische Gruppen | offen |

**Datenbank-Client** (Moritz, 14.11.2026): wird ein eigenes Programm nach V1, kein Teilplan von
SSHCraft. Server-Datenbanken erreicht er über einen Tunnel, den SSHCraft für ihn öffnet;
Einzelheiten im Neuaufbau-Entwurf, Abschnitt 16.3.

**Zielplattformen** (Moritz, 12.11.2026): Desktop Windows x64, x86, arm64, macOS universal,
Linux x86_64 und aarch64, FreeBSD x86_64; mobil Android, iOS und iPadOS. Teilplan 16 ist dafür
neu eingefügt [Claudes Entscheidung]; die bisherigen Teilpläne 16 bis 19 heißen jetzt 17 bis 20.

**Reihenfolge** (Moritz, 12.11.2026): erst Windows und Server, danach die übrigen
Desktop-Plattformen, dann Android, iOS und iPadOS. Teilplan 4
steht vorn [Claudes Entscheidung]: Der neue Aufbau und die Übersetzung kommen, bevor die vielen
neuen Bildschirme der Teilpläne 8 bis 14 entstehen, sonst müsste jeder davon zweimal angefasst
werden. Bewusst in Kauf genommen: Das Risiko der Handytastatur klärt sich erst mit Teilplan 18;
die Android-Werkzeuge (SDK, NDK 30.0.16248370, Rust-Ziele) sind schon installiert,
`ANDROID_HOME` ist `G:\Android\Sdk`.

Der Zweig `spike/android` enthält einen Tresor-Ersatz für Android, der den Hauptschlüssel als
Klartext-Datei ablegt. Er ist nur für den Spike gedacht und darf so nie nach `main`.

**winget** (Moritz, 12.11.2026): Die Windows-Versionen werden bei winget (Paketquelle
`microsoft/winget-pkgs`) eingetragen. Ein Workflow reicht bei jedem neuen Release das
aktualisierte Manifest automatisch ein; das gehört zu Teilplan 17.

**Nach Version 1** (Moritz, 12.11.2026): SFTP, Multiplayer, Import aus AWS, DigitalOcean und
Azure, KI-Funktionen, Mosh, Telnet. Seit 14.11.2026 haben SFTP (S1), Mosh und Telnet (F5) einen
Teilplan; Multiplayer, Cloud-Import und KI-Funktionen bleiben ohne Teilplan vorgemerkt.
