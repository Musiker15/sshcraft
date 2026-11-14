# SSHCraft: Vergleich mit anderen SSH-Programmen

Stand: 14.11.2026. Auftrag von Moritz: prüfen, welche Funktionen gängige SSH-Programme haben, die
SSHCraft nicht im Plan hat, und welche Schwächen sie haben, die SSHCraft vermeiden soll. Die
Einordnung jeder Zeile (Spalte "Plan") steht im Neuaufbau-Entwurf
[2026-11-14-sshcraft-craft-neuaufbau-design.md](2026-11-14-sshcraft-craft-neuaufbau-design.md),
Abschnitte 16 und 17.

Grundlage: offizielle Doku, Featureseiten, Changelogs und Sicherheitsmeldungen der Hersteller,
dazu Foren (Reddit, Hacker News), GitHub-Issues und Bewertungen. Je Programm ein bis zwei
Quellen. "(Einzelbericht)" heißt: nur eine Quelle, nicht bestätigt.

Geprüft: Termius, Tabby (früher "Terminus"), MobaXterm, SecureCRT und SecureFX, Xshell und Xftp,
Royal TS/TSX, Devolutions Remote Desktop Manager (RDM), WindTerm, Electerm, XPipe, Bitvise,
PuTTY und KiTTY, mRemoteNG, Ásbrú; die Terminals Warp, Wave, iTerm2, Windows Terminal, WezTerm,
Ghostty, kitty; mobil Blink Shell, JuiceSSH, Prompt 3, Secure ShellFish, ConnectBot, Termux,
Core Shell.

Spalte "Plan": **V1** = erste Version (Neuaufbau R1 bis R7 plus F1 und F2, Abschnitt 16.1 des Entwurfs), **F3** usw.
= Teilplan nach dem ersten Release, **M** = mobil, **später** = vorgemerkt ohne Teilplan,
**nein** = bewusst nicht.

## 1. Funktionen, die SSHCraft bisher nicht plant

### Verbindungen und Protokolle

| Funktion | Programme | Plan | Quelle |
|---|---|---|---|
| ProxyCommand (beliebiger Transportbefehl); Termius ignoriert ihn beim Import | WindTerm, SecureCRT, PuTTY | V1 | github.com/kingToolbox/WindTerm, vandyke.com/products/securecrt/features.html |
| Automatisches Wiederverbinden mit Backoff, Tunnel neu aufbauen, Netzwechsel und Ruhezustand erkennen | Bitvise, Tabby (nur seriell), RDM (seit 2025.3.28) | V1 | bitvise.com/ssh-client, forum.devolutions.net/topics/51447 |
| Verbindung wiederverwenden (ControlMaster): weitere Tabs zum selben Host ohne neue Anmeldung | kitty, iTerm2 | F4 | sw.kovidgoyal.net/kitty/kittens/ssh/ |
| Befehl vor und nach der Verbindung (Wake-on-LAN, Port-Knocking, VPN) | PuTTY 0.84, Ásbrú, Royal TS | F4 | chiark.greenend.org.uk/~sgtatham/putty/changes.html, github.com/asbru-cm/asbru-cm |
| X11-Weiterleitung (mit einem vorhandenen X-Server, z. B. WSLg oder XQuartz) | MobaXterm, Tabby, WindTerm, SecureCRT, Xshell, XPipe | F4 | mobaxterm.mobatek.net/features.html |
| Weiterleitung von Unix-Sockets | Bitvise | F4 | bitvise.com/ssh-client |
| `ssh://`-Links über den System-Handler | Electerm | F4 (streng geprüft, Abschnitt 4 unten) | github.com/electerm/electerm |
| Erreichbarkeit prüfen (Ping, TCP) und als Punkt in der Hostliste | mRemoteNG, Royal TS, SecureCRT | F4 | royalapps.com/ts/win/features |
| Schnellverbinden: `benutzer@host:port` in die Suche tippen | Xshell 8, Termius | F4 | docs.termius.com/changelog |
| Dauerhafte Sitzungen: überleben Netzwechsel, Ruhezustand und Neustart des Clients, Zustand im Tab | Wave 0.14, WezTerm (Remote-Mux) | F5 | docs.waveterm.dev/releasenotes, wezterm.org/multiplexing.html |
| tmux-Steuermodus: tmux-Fenster als echte Tabs und Bereiche | iTerm2 (tmux -CC), WindTerm, XPipe | F5 | iterm2.com/documentation-tmux-integration.html |
| Eternal Terminal (wie Mosh, mit vollem Scrollback) | Prompt 3 | F5 | help.panic.com/prompt/prompt3 |
| Dateien über das Terminal (ZMODEM, trzsz, kitty transfer mit rsync-Deltas) | Tabby, WindTerm, Electerm, SecureCRT, kitty | F6 (nur auf Rückfrage) | sw.kovidgoyal.net/kitty/kittens/transfer/ |
| Container als Ziel (Docker, Podman, LXD, Kubernetes, Proxmox) | XPipe | F7 | xpipe.io |
| Weiterleitung mit Startaktion (RDP, VNC oder Browser über den Tunnel öffnen) | Bitvise, Royal TS, XPipe | F7 | bitvise.com/ssh-client |
| RDP, VNC, Spice als eigene Tabs | MobaXterm, Xshell 8, Royal TS, RDM, mRemoteNG, Electerm | nein (SSHCraft bleibt SSH-Client; Startaktion reicht) | royalapps.com/ts/win/features |
| Verschleiertes SSH, FTP-zu-SFTP-Brücke | Bitvise | nein | bitvise.com/ssh-client |

### Terminal

| Funktion | Programme | Plan | Quelle |
|---|---|---|---|
| Shell-Integration (OSC 133): Marke je Prompt, Sprung zwischen Befehlen, Exit-Code, Laufzeit und Verzeichnis je Befehl, Ausgabe eines Befehls markieren, Marken in der Scrollleiste | iTerm2, Windows Terminal, Ghostty | F3 | iterm2.com/documentation-shell-integration.html, learn.microsoft.com/windows/terminal/tutorials/shell-integration |
| Ausgabe je Befehl falten, Zeitstempel je Zeile | WindTerm | F3 | github.com/kingToolbox/WindTerm |
| Befehlsblöcke: Befehl und Ausgabe als Einheit kopieren, teilen, erneut ausführen | Warp | F3 | docs.warp.dev/terminal/blocks |
| Umgebung beim Verbinden einspielen (terminfo, Shell-Integration), ohne Installation auf dem Server | kitty, Ghostty, iTerm2, Wave | F3 (nichts dauerhaft auf dem Server, Abschnitt 4) | sw.kovidgoyal.net/kitty/kittens/ssh/ |
| Meldung, wenn ein langer Befehl fertig ist; Meldungen vom Server (OSC 9/777, BEL) | iTerm2, Tabby, Secure ShellFish, Blink | F3 | docs.blink.sh/basics/notifications |
| Trigger: Regex auf die Ausgabe löst aus (hervorheben, Meldung, Text senden, Marke setzen) | iTerm2, Xshell, Tabby, Electerm, KiTTY | F3 | iterm2.com/documentation-triggers.html |
| Schlüsselwort-Hervorhebung, Sätze je Sitzung | SecureCRT, Xshell, MobaXterm | F3 | vandyke.com/products/securecrt/features.html |
| Farbe, Theme oder Abzeichen je Host oder Gruppe (z. B. Produktion rot), automatischer Profilwechsel | iTerm2, kitty | F3 | iterm2.com/documentation-shell-integration.html |
| Hinweismodus: URLs, Pfade, Hashes, IPs per Tastatur wählen und öffnen oder kopieren | kitty, iTerm2 (Smart Selection) | F3 | sw.kovidgoyal.net/kitty/kittens/hints/ |
| Suche mit regulären Ausdrücken im Scrollback | iTerm2, WezTerm | V1 (gehört ins neue Terminal) | wezterm.org/features.html |
| Einfüge-Verlauf | iTerm2 | F3 | iterm2.com/features.html |
| Compose-Bereich: mehrzeiliger Entwurf an eine Sitzung, alle oder eine Gruppe | Xshell, SecureCRT | F3 | op-www.netsarang.com/products/xsh_key_features.html |
| Knopfleiste mit Befehlen | SecureCRT, Xshell | F3 (Snippets als Knöpfe) | vandyke.com/products/securecrt/features.html |
| Anmeldeskripte (Erwarten und Senden nach dem Login) | Tabby, Ásbrú, Xshell, KiTTY | F3 | github.com/Eugeny/tabby |
| Snippets mit Platzhaltern, die beim Ausführen abgefragt werden | Royal TS, Ásbrú, Warp | F3 | docs.warp.dev/knowledge-and-collaboration/warp-drive |
| Bilder im Terminal (Sixel, Kitty Graphics, iTerm2-Protokoll) | WezTerm, Ghostty, iTerm2 | F6 | wezterm.org/features.html |
| Ligaturen, Graphem-Cluster (Flaggen, Emoji mit Hautfarbe) | Ghostty, WezTerm | V1 (Anforderung ans neue Terminal) | ghostty.org/docs/features |
| Hex-Ansicht empfangener Bytes (seriell) | Xshell 8, SecureCRT, Tabby | F6 | netsarang.atlassian.net/wiki/x/AYBOhQ |
| Hotkey- bzw. Dropdown-Fenster | iTerm2, Ghostty, Tabby, Electerm | F6 | ghostty.org/docs/features |
| Sichere Tastatureingabe (macOS) | Ghostty | F4 | ghostty.org/docs/features |
| Ressourcenleiste (CPU, RAM, Netz, Platte), abschaltbar | MobaXterm | F4 (Vorgabe aus) | blog.mobatek.net/post/mobaxterm-new-release-10.6 |
| Notebooks mit ausführbaren Befehlsblöcken | Warp | später | docs.warp.dev |
| Eingabe lokal editieren (Vim-Modus) | WindTerm | nein | github.com/kingToolbox/WindTerm |

### Organisation

| Funktion | Programme | Plan | Quelle |
|---|---|---|---|
| `~/.ssh/config` laufend lesen und auf Wunsch zurückschreiben (mit `Include`) statt einmaligem Import | XPipe, Windows Terminal (ohne Include) | V1 | xpipe.io |
| Sammelbearbeitung vieler Hosts | SecureCRT, Royal TS, Xshell | F4 | royalapps.com/ts/win/features |
| Portabler Modus | Tabby, Bitvise, KiTTY, XPipe | F4 (craft-settings kann ihn) | github.com/cyd01/KiTTY |
| Desktop-Verknüpfung zu einem Host | Bitvise, XPipe | F4 | bitvise.com/ssh-client |
| Dynamische Gruppen (ein Befehl liefert die Hostliste) | Royal TS | F7 | royalapps.com/ts/win/features |
| Sync über Git, Gist oder WebDAV | XPipe, Electerm, Tabby | nein (eigener Sync-Server) | xpipe.io |

### Sicherheit und Anmeldung

| Funktion | Programme | Plan | Quelle |
|---|---|---|---|
| Passwort als Verweis auf einen Passwortmanager (`op://…`, Bitwarden, KeePassXC, Proton Pass), geholt beim Verbinden | XPipe, Royal TS, Ásbrú | V1 | docs.xpipe.io/guide/password-manager |
| TOTP für keyboard-interactive | Bitvise, Electerm, kitty (askpass) | V1 | bitvise.com/ssh-client |
| Hostkey-Verwaltung: ansehen, importieren, exportieren; Schlüsselwechsel des Servers annehmen (`UpdateHostKeys`) | SecureCRT, Bitvise | V1 | vandyke.com/products/securecrt/features.html |
| App-Sperre auf dem Desktop nach Leerlauf (Windows Hello, Touch ID) | WindTerm, SecureCRT, Xshell, MobaXterm | V1 | github.com/kingToolbox/WindTerm |
| Smartcards (PIV, PKCS#11) | SecureCRT, Bitvise, XPipe, WindTerm | F7 | vandyke.com/products/securecrt/features.html |
| Kerberos (GSSAPI) | SecureCRT, Bitvise | F7 | bitvise.com/ssh-client |
| FIPS-Modus | SecureCRT, Bitvise | nein | vandyke.com |

### Automatisierung und Zusammenarbeit

| Funktion | Programme | Plan | Quelle |
|---|---|---|---|
| Makros aufnehmen und auf anderen Hosts abspielen | SecureCRT, Xshell, MobaXterm | F7 (aus dem Befehls-Journal der Engine) | vandyke.com/products/securecrt/features.html |
| Skripte gegen die App | SecureCRT, Xshell, Royal TS, RDM | F7 (`sshcraft-cli run --script`) | docs.devolutions.net/rdm/concepts/advanced-concepts |
| Teilen mit Platzhalter für Zugangsdaten: jeder Empfänger setzt seine eigene Identität ein | Royal TS | F4 | royalapps.com/ts/win/features |
| Agent-Status in der Liste (arbeitet, wartet auf Eingabe, fertig), OSC 7501 | Termius 6.3 bis 8.2 | F3 | docs.termius.com/changelog/ios |
| Check-out, Vier-Augen-Freigabe, Rollen, SSO, Video-Aufzeichnung | RDM, Termius Business | nein | devolutions.net/remote-desktop-manager/features/ |
| Plugins | Tabby, MobaXterm, mRemoteNG | später | github.com/Eugeny/tabby |
| Selbst gehostete Web-Fassung | Tabby | später (eigenes Vorhaben, Neuaufbau-Entwurf Abschnitt 2) | github.com/Eugeny/tabby |

## 2. SFTP

Moritz, 14.11.2026: SFTP kommt in den Plan, muss aber nicht in den ersten Release (Teilplan S1).
Maßstab für die Übertragung ist SecureFX, für die Verzahnung mit dem Terminal Termius und
MobaXterm.

| Funktion | Am besten gelöst | Quelle |
|---|---|---|
| Zwei Bereiche lokal und entfernt, auch Server zu Server | Xftp (FXP), Termius (Host zu Host per Ziehen), SecureFX | netsarang.com/en/xftp/, docs.termius.com/organize-and-connect-to-hosts/managing-files-with-sftp |
| Dateibrowser folgt dem Verzeichnis des Terminals; "Terminal hier öffnen" | MobaXterm ("Follow SSH path"), Tabby, XPipe | mobaxterm-en.mobatek.net/documentation.html |
| Bearbeiten mit automatischem Hochladen, Standard-App je Dateityp | Termius, SecureFX, MobaXterm, XPipe | docs.termius.com/organize-and-connect-to-hosts/managing-files-with-sftp |
| Warteschlange mit Pause, Fortsetzen nach Abbruch | SecureFX, Xftp | vandyke.com/products/securefx/features.html |
| Fortsetzen mit Prüfsumme (SFTP `check-file`) | Bitvise | bitvise.com/ssh-client |
| Parallele Übertragung, Bandbreitengrenze, Regeln beim Überschreiben | SecureFX, Xftp | vandyke.com/products/securefx/features.html |
| Rechte (chmod) für viele Dateien, Rechte beim Hochladen | SecureFX | vandyke.com/products/securefx/features.html |
| Vergleichen, Synchronisieren, Spiegeln als gespeicherter Auftrag | SecureFX, Xftp, Bitvise | netsarang.com/en/xftp/ |
| Synchrones Blättern, Lesezeichen, Filter, Suche | SecureFX, Xftp | vandyke.com/products/securefx/features.html |
| `sudo` im Dateibrowser | XPipe | xpipe.io |
| SCP als Rückfall ohne SFTP-Subsystem | SecureFX, WindTerm, Royal TS | vandyke.com/products/securefx/features.html |
| Ziehen aus dem Dateimanager des Systems ins Terminal lädt ins aktuelle Verzeichnis hoch | iTerm2, kitty, Termius | iterm2.com/features.html |

Bewusst nicht: FTPS, WebDAV, S3, SFTP als Windows-Laufwerk, geplante Übertragungen.

## 3. Mobil (für die Teilpläne M1 bis M3)

| Funktion | Programme | Quelle |
|---|---|---|
| Mosh und Eternal Terminal, Sitzung überlebt Wechsel ins Mobilnetz | Blink, Termius, Prompt 3 | help.panic.com/prompt/prompt3 |
| iOS: Verbindung im Hintergrund länger halten (Standortdienst als Opt-in, Live Activities) | Blink, Termius | docs.termius.com/help-center/faq |
| Server als Ort in der Dateien-App, offline bearbeiten | Secure ShellFish, Blink | secureshellfish.app, docs.blink.sh/advanced/files-app |
| Benachrichtigungen vom Server (OSC 9/777, BEL als Vibration, Befehl aus Cron) | Secure ShellFish, Blink | docs.blink.sh/basics/notifications |
| Widgets, Kurzbefehle (Shortcuts), Tasker | Secure ShellFish, Prompt 3, Termux:Widget | github.com/termux/termux-widget |
| Hochladen aus anderen Apps über das Teilen-Menü, Pfad landet im Terminal | Termius 7.5, Secure ShellFish | docs.termius.com/changelog/ios |
| Hardware-Tastatur mit eigenen Kürzeln, Kitty-Tastaturprotokoll, CJK-Eingabe | Blink, Termius | docs.blink.sh |
| Eigene Bildschirmtastatur, frei belegbare Zusatzleiste | Prompt 3; ConnectBot kann es nicht (Einzelbericht) | help.panic.com/prompt/prompt3 |
| Touch als Eingabe für tmux und mausfähige Programme | Secure ShellFish, Blink | secureshellfish.app |
| YubiKey PIV über USB und NFC | ConnectBot 1.12, Core Shell | github.com/connectbot/connectbot/releases |
| Mehrere Fenster auf dem iPad | Prompt 3 | help.panic.com/prompt/prompt3 |

## 4. Schwächen anderer Programme und wie SSHCraft sie vermeidet

Spalte "SSHCraft": **erfüllt** = steht schon im Design und hat Tests; sonst der Ort im Plan.

| Schwäche | Programme | Beleg | SSHCraft |
|---|---|---|---|
| **Preis und Lizenz** | | | |
| Sync und Teilen nur im Abo, Team-Tresor 20 $ je Nutzer und Monat | Termius | termius.com/pricing | erfüllt: Sync und Teilen kostenlos über den eigenen Server |
| Gekaufte Lizenzen gelten nicht mehr, Einmalkauf eingestellt | JuiceSSH (Dez. 2025), Blink | news.ycombinator.com/item?id=46768909, docs.blink.sh/migration | erfüllt: MIT OR Apache-2.0, keine Lizenzprüfung |
| Künstliche Grenzen der Gratisfassung | MobaXterm Home (12 Sitzungen, 2 Tunnel), XPipe | mobaxterm-en.mobatek.net/license.html | erfüllt: keine gesperrten Funktionen |
| Teure Wartung, Preiserhöhungen | SecureCRT, Royal TS | vandyke.com/pricing | erfüllt |
| **Datenschutz und Cloud-Zwang** | | | |
| Konto oder Hersteller-Cloud nötig | Termius, Prompt, Warp (bis Nov. 2024) | warp.dev/blog/lifting-login-requirement | erfüllt: kein Konto für die App, Sync optional und selbst betrieben |
| Telemetrie ab Werk, nach Neustart wieder an | Warp, Wave | news.ycombinator.com/item?id=47945823, docs.waveterm.dev/telemetry | V1: keine Telemetrie; jede Verbindung, die SSHCraft selbst aufbaut, steht in einer Liste in den Einstellungen; Update-Prüfung nur nach Zustimmung |
| KI schickt Terminalinhalte ohne klare Zustimmung | Warp "Active AI", iTerm2 3.5.0 | news.ycombinator.com/item?id=44953470 | erfüllt für Agents (Rechte, Rückfrage, Protokoll); KI-Funktionen später nur ab Werk aus |
| Sync-Dienst eingestellt, Daten weg | Tabby (app.tabby.sh), JuiceSSH (Einzelbericht) | github.com/Clem-Fern/rtabby-web-api | erfüllt: eigener Server; V1: verschlüsselte Sicherung als Datei |
| **Sicherheit** | | | |
| Hintertür in signiertem Update (ShadowPad 2017) | Xshell, Xmanager | csoonline.com/article/3216547 | V1: offener Code, signierte Pakete, SBOM über `supply-chain.yml`; reproduzierbare Builds als Ziel |
| Kryptofehler: P-521-Schlüssel aus rund 60 Signaturen (CVE-2024-31497), Terrapin (CVE-2023-48795) | PuTTY, KiTTY, SecureCRT, Xshell, FileZilla, WinSCP | openwall.com/lists/oss-security/2024/04/15/6, terrapin-attack.com/patches.html | V1: Krypto nur aus geprüften Bibliotheken mit deterministischen Nonces (RFC 6979), Test auf strict-kex gegen Terrapin |
| Gespeicherte Passwörter schwach geschützt (Null-IV, nur verschleiert) | MobaXterm < 25.0 (CVE-2025-0714), KiTTY, RDM (CVE-2025-13683) | nvd.nist.gov/vuln/detail/CVE-2025-0714 | erfüllt: XChaCha20-Poly1305, Hauptschlüssel im Schlüsselbund des Systems, Windows Hello |
| Codeausführung über Deep-Links (`tabby://run`), ZMODEM-Autostart beim `cat`, SSH-Agent ohne Prüfung, Pfad-Traversal im SFTP, Befehlseinschleusung über Dateinamen, OSC-52-Leck | Tabby (9 Advisories 2024 bis 2026), Electerm 2026, Termius (CVE-2024-57061) | github.com/Eugeny/tabby/security/advisories, github.com/electerm/electerm/security/advisories | V1, Regeln in Abschnitt 17 des Entwurfs: kein Shell-String aus fremden Daten, Links öffnen nur Verbindungsdialoge, Dateiübertragung über das Terminal nie automatisch, OSC 52 nur schreiben und nur nach Zustimmung je Host, Pfade aus dem Netz normalisiert und begrenzt |
| MCP-Schnittstelle ohne Rechteprüfung, HTTP-Server ohne Origin-Prüfung (DNS-Rebinding) | Electerm (Nov. 2026) | electerm-Advisories GHSA-vw4q-c6px-w5p3, GHSA-45rj-2wq9-r9mr | erfüllt: Rechte je Aktion, Rückfrage im Fenster; V1: kein HTTP, Token an jeder Verbindung |
| Client schreibt Sitzungsinhalt auf den Server (`/tmp/framer.txt`, CVE-2025-22275) | iTerm2 3.5.6 bis 3.5.10 | openwall.com/lists/oss-security/2025/01/03/2 | V1-Regel: nichts dauerhaft auf dem Zielsystem ablegen; das Einspielen in F3 lebt nur im Speicher der Sitzung |
| Gefälschte Downloads über Suchanzeigen | PuTTY, WinSCP | rapid7.com/blog/post/2024/05/13/ongoing-malvertising-campaign-leads-to-ransomware | V1: eine offizielle Download-Adresse, signierte Pakete über winget, Homebrew, Flathub, Prüfsummen |
| Sicherheitsfix unvollständig, zweites Advisory nötig | Tabby | GHSA-mq9v-2pgm-fxgh | erfüllt: jede Lücke bekommt einen Test, der vorher fehlschlug (House rules) |
| **Leistung** | | | |
| Electron frisst Speicher | Tabby, Termius, Wave, Electerm | reddit.com/r/unix/comments/1k5t4d4 | erfüllt durch Rust und egui; V1: Speicherbudget als Test |
| Große Ausgabe blockiert den Tab | Tabby, Windows Terminal, MobaXterm (Einzelbericht) | github.com/microsoft/terminal/issues/8640 | erfüllt: Flusskontrolle; V1: Budget "1 GB `cat` ohne Hänger der Oberfläche" als Test |
| **Stabilität** | | | |
| Kein automatisches Wiederverbinden, Hänger nach dem Ruhezustand | RDM, Tabby, MobaXterm (Einzelbericht) | forum.devolutions.net/topics/51447 | V1: Wiederverbinden mit Backoff, Tunnel neu, Netzwechsel und Ruhezustand erkennen |
| **Bedienung** | | | |
| Kein Sitzungsmanager | Windows Terminal | github.com/microsoft/terminal/issues/13327 | erfüllt |
| Keine Tabs, Sitzungen nur in der Registry | PuTTY | chiark.greenend.org.uk/~sgtatham/putty/wishlist/multiple-connections.html | erfüllt |
| `ssh_config`-Import unvollständig (ProxyCommand ignoriert) | Termius | termius.com/documentation/import-from-ssh-config | V1: `~/.ssh/config` vollständig mit `Include` und ProxyCommand |
| FIDO2-Schlüssel spät oder gar nicht | RDM, Termius (Einzelbeleg) | forum.devolutions.net/topics/40374 | erfüllt |
| **Plattformen** | | | |
| Nur ein Betriebssystem; kein GUI-Client mit offiziellem FreeBSD-Build gefunden | MobaXterm, Xshell, Bitvise, iTerm2, Blink, JuiceSSH u. a. | Herstellerseiten | V1: Windows, macOS, Linux, FreeBSD mit gleichem Umfang |
| **Sync** | | | |
| Tresor plötzlich leer, nach Update alle Verbindungen weg | Termius (Einzelberichte) | reddit.com/r/sysadmin/comments/1inocsl | V1: lokale Daten bleiben maßgeblich, Papierkorb und Versionen, Sync löscht lokal nie ohne Grabstein und Papierkorb |
| Verschlüsselungspasswort weg heißt Daten weg | Termius (so gewollt) | docs.termius.com/help-center/faq | erfüllt: Kontoschlüssel in Proton Pass, jedes Gerät mit eigenem Schlüssel; V1: verschlüsselte Sicherung als Datei |
| **Mobil** | | | |
| iOS beendet Sitzungen im Hintergrund nach 20 bis 30 s | alle iOS-Clients | docs.termius.com/help-center/faq | M: Mosh, Eternal Terminal, Live Activities |
| Zusatzleiste nicht einstellbar | ConnectBot (Einzelbericht) | news.ycombinator.com/item?id=46768909 | M: frei belegbare Leiste |
| **Barrierefreiheit** | | | |
| SSH-Fenster für Screenreader unlesbar | MobaXterm, Bitvise (Test einer Hochschule) | support.beocat.ksu.edu/Docs/Accessibility | V1: AccessKit über egui, Terminal als linearer Text lesbar |
| **Projektpflege** | | | |
| Verwaist, Support antwortet nicht | JuiceSSH (letztes Update 2021) | appbrain.com/app/juicessh-ssh-client/com.sonelli.juicessh | Ziel Craft-Familie: mehrere Maintainer, offenes Datenformat |
| Ein Entwickler, rund 2.000 offene Issues | WindTerm | github.com/kingToolbox/WindTerm/issues/1596 | wie oben |

### Die zehn wichtigsten Schwächen

1. Abo für Grundfunktionen wie Sync und Teilen.
2. Konto- und Cloud-Zwang ohne eigenen Server.
3. Lücken mit Codeausführung in Electron-Clients.
4. Schwach geschützte Passwörter und Schlüssel.
5. Verfallende Lizenzen und verwaiste Apps.
6. KI und Telemetrie senden ungefragt.
7. Ressourcenhunger und Hänger bei großer Ausgabe.
8. Datenverlust beim Sync ohne Wiederherstellung.
9. Abbrüche ohne automatisches Wiederverbinden.
10. Lock-in: ein Betriebssystem, unvollständiger Import und Export, Teilen nur im teuren Tarif.

Zur Quellenlage: Bei Stabilität, großer Ausgabe und Sync-Konflikten gibt es fast nur
Einzelberichte. Die Sicherheitsbelege sind CVEs und Advisories der Projekte selbst.
