# SSHCraft: Fahrplan der Teilpläne

Design: `docs/superpowers/specs/2026-11-11-sshcraft-design.md` (freigegeben am 11.11.2026, Umfang
erweitert am 12.11.2026 auf den Funktionsumfang von Termius ohne SFTP, dazu Englisch und Deutsch).

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
| 8 | Hosts und Ordnung | Untergruppen, Tags, Host-Symbole, Import aus `known_hosts`, Schlüsseldateien, CSV, PuTTY, MobaXterm, SecureCRT, Export als CSV und JSON | offen |
| 9 | Verbindungen | Passwort und keyboard-interactive, Identitäten, Sprung-Hosts, HTTP- und SOCKS5-Proxy, Agent-Weiterleitung, Umgebungsvariablen, serielle Verbindung, lokales Terminal | offen |
| 10 | Schlüssel erweitert | ECDSA und RSA erzeugen, Passphrase speichern, öffentlichen Schlüssel in `authorized_keys` eintragen, SSH-Zertifikate, FIDO2-Hardwareschlüssel, gerätegebundene Schlüssel (Windows Hello mit TPM) | offen |
| 11 | Workspaces | Split und Fokus mit 2 bis 16 Sitzungen, Broadcast, Workspace-Vorlagen, Statuspunkte, Tastenkürzel, Terminaltyp, OSC 52, Links | offen |
| 12 | Portweiterleitung | lokal, entfernt, dynamisch (SOCKS), Regelseite, Assistent | offen |
| 13 | Snippets und Autovervollständigung | Snippets in Paketen, auf mehreren Hosts ausführen, Seitenleiste mit Verlauf, Autovervollständigung aus Verlauf, Pfaden, Snippets und Befehlen (ohne KI) | offen |
| 14 | Sitzungsprotokolle | Aufzeichnung, Lesezeichen, Kommentare, Suche, Löschen, Export, Aufbewahrung, Sync | offen |
| 15 | Agent-Anbindung | `sshcraft mcp`, Kommandozeile `sshcraft`, Rechte pro Aktion, Bestätigungsfenster, Ausblenden, Protokoll | offen |
| 16 | Desktop-Plattformen | macOS, Linux und FreeBSD: Agent über `SSH_AUTH_SOCK`, Keychain und Secret Service, Touch ID, Login-Shell als lokales Terminal, `pass-cli`-Pfade; CI-Matrix für alle Desktop-Ziele | offen |
| 17 | Releases für Desktop und Server | GitHub-Actions-Releases: Windows x64, x86, arm64; macOS universal; Linux x86_64 und aarch64; FreeBSD x86_64; Server-Programm für Linux; Docker-Image | offen |
| 18 | Android-Spike (Wegwerf) | Urteil zur Handytastatur mit xterm.js im Webview | angefangen, Zweig `spike/android` |
| 19 | Android vollständig | Keystore, Zusatzleiste, Gesten, Lautstärketasten, Vordergrund-Dienst, App-Sperre, gerätegebundene Schlüssel, APK in den Releases | offen |
| 20 | iOS und iPadOS | Keychain, Face ID, Secure Enclave, Wiederverbinden beim Zurückkehren, iPad-Layout mit Seitenleiste, Hardwaretastatur und Split View, IPA-Bau auf macOS-Runner, Anleitung zum Sideloading | offen |

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

**Nach Version 1** (Moritz, 12.11.2026): SFTP, Multiplayer, Import aus AWS, DigitalOcean und
Azure, KI-Funktionen, Mosh, Telnet.
