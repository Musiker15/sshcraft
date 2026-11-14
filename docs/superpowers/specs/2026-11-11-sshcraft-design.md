# SSHCraft: Design

Stand: 11.11.2026, Umfang erweitert am 12.11.2026 (Abschnitt 2, "Funktionsumfang nach
Termius-Vorbild"). Ergebnis des Brainstormings mit Moritz Kohm (GitHub `musiker15`).
Entscheidungen von Moritz stehen ohne Zusatz. Was ich (Claude) entschieden habe, weil es
offen blieb, ist mit **[Claudes Entscheidung]** markiert und darf beim Lesen widersprochen
werden.

> **Seit 14.11.2026:** Abschnitt 3 (Technik) und der Transport in Abschnitt 10 sind ersetzt
> durch [2026-11-14-sshcraft-craft-neuaufbau-design.md](2026-11-14-sshcraft-craft-neuaufbau-design.md)
> (Rust und egui nach den Konventionen der Craft-Apps). SFTP und weitere Funktionen aus dem
> [Marktvergleich](2026-11-14-sshcraft-marktvergleich.md) stehen dort in Abschnitt 16.

## 1. Worum es geht und warum

SSHCraft ist ein SSH-Client für den Desktop (Windows, macOS, Linux, FreeBSD) und für
Mobilgeräte (Android, iOS, iPadOS) mit eigenem, selbst betriebenem Sync-Server. Vorbild ist Termius: SSHCraft soll ihm in Aussehen und Funktionen weitgehend
entsprechen, nur verbessert und angepasst. Der Name folgt den vielen Nachbauten auf GitHub, die
"Craft" im Namen tragen (Moritz, 12.11.2026; vorher hieß das Projekt "Stollen").

Der Anlass: Termius verlangt monatliche Gebühren, damit Hosts und Schlüssel zwischen Geräten
synchronisiert werden. Ohne Abo muss auf jedem neuen Gerät alles neu angelegt werden, und
besonders lästig ist, dass die SSH-Schlüssel jedes Mal als Datei auf das Gerät kopiert werden
müssen.

Moritz hält seine SSH-Schlüssel bereits in Proton Pass. Auf dem Windows-PC ist die
Proton-Pass-Desktop-App der SSH-Agent (Named Pipe `\\.\pipe\openssh-ssh-agent`). Auf Android
und iOS bietet Proton Pass anderen Apps keine Schnittstelle an. Genau dafür ist der Sync da:
er bringt die Schlüssel auf die Handys.

SSHCraft wird ein öffentliches Projekt auf GitHub im Konto `musiker15`, Lizenz **AGPL-3.0**.
Andere sollen es mit ihrem eigenen Server betreiben können.

## 2. Ziele, Nicht-Ziele, Anti-Ziele

### Ziele (alles davon gehört in die erste Version)

- SSH-Terminal auf allen Zielplattformen (Abschnitt 3, "Zielplattformen").
- Hosts und Gruppen verwalten.
- Schlüssel aus drei Quellen: Proton-Pass-Agent, Proton Pass über `pass-cli`, eigener Schlüssel.
- Anbindung für KI-Agents über MCP und Kommandozeile, mit Rechten pro Aktion und ohne Zugriff
  auf private Schlüssel.
- Optionaler Sync über einen oder mehrere selbst betriebene Server, Ende-zu-Ende-verschlüsselt.
- Hosts und Gruppen mit anderen Benutzern desselben Servers teilen, mit oder ohne Schlüssel,
  lesend oder schreibend, live aktualisiert.
- Zusatzleiste über der Handy-Tastatur.
- App-Sperre auf dem Handy per PIN, Fingerabdruck oder Face ID.
- Der Funktionsumfang von Termius ohne SFTP, siehe "Funktionsumfang nach Termius-Vorbild".
- Oberfläche auf Englisch und Deutsch, Vorgabe AUTO.

Erfolg heißt: Mit der ersten Version kann Moritz Termius kündigen.

### Nicht-Ziele

- **Später, nicht in der ersten Version** (Moritz, 12.11.2026): SFTP, Multiplayer (mehrere
  Personen im selben Terminal), Import aus AWS, DigitalOcean und Azure, KI-Funktionen, Mosh,
  Telnet.
- Bis 12.11.2026 stand hier "Nie: SFTP" und "später: Portweiterleitung, Sprung-Hosts,
  Agent-Weiterleitung, Mosh, Snippets, Sitzungsaufzeichnung"; das gilt nicht mehr.
- Keine Web-Oberfläche für den Server-Betreiber, Verwaltung nur per Befehl.

### Anti-Ziele (wäre ein Fehlschlag, auch wenn es technisch läuft)

- Der Server kann gespeicherte SSH-Schlüssel oder Host-Daten lesen, auch nur im Fall eines
  Einbruchs auf dem Server.
- Ein privater Schlüssel liegt unverschlüsselt als Datei auf einem Gerät.
- Sync ist Pflicht oder lässt sich nicht fein einstellen.

### Funktionsumfang nach Termius-Vorbild (Version 1)

Moritz, 12.11.2026: SSHCraft soll Termius in Aussehen und Funktionen weitgehend entsprechen, nur
verbessert und angepasst; SFTP kommt später, alles andere gehört in Version 1. Grundlage ist eine
Bestandsaufnahme der Termius-Doku (docs.termius.com, Stand November 2026). Was dort an Termius'
eigener Cloud hängt, ersetzt der selbst betriebene Sync-Server.

**Verbindungen**
- SSH mit Schlüssel **und Passwort** (auch keyboard-interactive); Passwörter liegen verschlüsselt
  im Tresor und werden wie Schlüssel synchronisiert.
- **Identitäten:** wiederverwendbare Bündel aus Benutzer, Passwort und/oder Schlüssel für Hosts
  und Gruppen.
- Sprung-Hosts (Host-Ketten) je Host oder Gruppe, von Gruppen vererbt.
- HTTP- und SOCKS5-Proxy je Host, mit optionalen Zugangsdaten.
- Agent-Weiterleitung.
- Umgebungsvariablen je Host.
- **Seriell** (nur Desktop): Baudrate, Datenbits, Stoppbits, Parität, Flusskontrolle.
- **Lokales Terminal** (nur Desktop): PowerShell, cmd, WSL oder eine eingestellte Shell als Tab.
- Post-Quanten-Schlüsseltausch und ML-DSA, sobald `russh` sie unterstützt
  [Claudes Entscheidung, hängt an der Bibliothek].

**Terminal und Arbeitsfläche**
- Tabs, Befehlspalette (Hosts zuerst), Neuer-Tab-Seite mit Suche und letzten Verbindungen.
- **Workspaces** (Desktop): 2 bis 16 Sitzungen nebeneinander (Split) oder als Hauptansicht mit
  Seitenleiste (Fokus), Bereiche ziehen und in der Größe ändern; **Broadcast** einer Eingabe an
  alle Terminals des Workspace; **Workspace-Vorlagen**, die Verbindungen, Namen, Anordnung,
  Arbeitsverzeichnis und laufenden Befehl merken.
- Statuspunkte je Bereich (neue Ausgabe, Eingabe erwartet, Fehler).
- Themes und Schriften, Terminaltyp wählbar (z. B. xterm-256color, linux, vt100), OSC 52
  (Zwischenablage), Links mit Strg-Klick öffnen, frei belegbare Tastenkürzel.
- Seitenleiste im Terminal mit Snippets und Verlauf.
- **Autovervollständigung** aus Verlauf, Pfaden, Snippets und bekannten Befehlen samt Argumenten,
  ohne KI.
- Handy: Zusatzleiste (Abschnitt 9), dazu Gesten (Wischen für Pfeiltasten, Zwei-Finger-Zoom) und
  Lautstärketasten als Tasten.

**Hosts und Ordnung**
- Gruppen mit **Untergruppen** (verschachtelt), Vererbung über alle Ebenen, **Tags**,
  Host-Symbole nach Betriebssystem oder Distribution.
- **Import:** `~/.ssh/config` (vorhanden), `known_hosts`, Schlüsseldateien, CSV, PuTTY (unter
  Windows direkt aus der Registry, sonst `.reg`), MobaXterm, SecureCRT.
- **Export** von Hosts und Gruppen als CSV und JSON, ohne Geheimnisse
  [Claudes Entscheidung, bei Termius nicht dokumentiert, gehört zu "verbessert"].

**Schlüssel**
- Erzeugen und Importieren von Ed25519, ECDSA und RSA, Passphrase wahlweise speichern.
- Öffentlichen Schlüssel per Klick in `authorized_keys` des Servers eintragen.
- SSH-Zertifikate, gespeichert zusammen mit dem privaten Schlüssel.
- **FIDO2-Hardwareschlüssel** (YubiKey und ähnliche), mit Berührung und PIN.
- **Gerätegebundene Schlüssel**, die das Gerät nie verlassen und nicht synchronisiert werden:
  Windows Hello (TPM), Android Keystore, iOS Secure Enclave.

**Portweiterleitung**
- Lokal, entfernt und dynamisch (SOCKS), Regeln auf einer eigenen Seite, Start und Stopp per
  Doppelklick, Assistent zum Anlegen.

**Snippets**
- Snippets in Paketen (Ordnern), auf mehreren Hosts ausführen (neue Verbindungen oder alle
  offenen Tabs), Standardziele je Snippet, Snippets in der Autovervollständigung.

**Sitzungsprotokolle**
- Aufzeichnung je Host für SSH, seriell und lokales Terminal, Lesezeichen und Kommentare,
  durchsuchbar, verschlüsselt und mit Sync; Ansicht auf dem Desktop.
- Anders als bei Termius: Protokolle lassen sich löschen und exportieren, die Aufbewahrung ist
  einstellbar [Claudes Entscheidung, "verbessert"].

**Aussehen**
- Aufbau und Bedienung eng an Termius angelehnt (Seitenleiste mit Bereichen für Hosts,
  Schlüssel, Snippets, Portweiterleitung und Protokolle; Tabs; Dialoge; Abläufe).
- Logo, Icons, Farben, Grafiken und Texte sind eigene Gestaltung, keine Kopie von Termius
  [Claudes Entscheidung: das Aussehen von Termius ist geschütztes Design, und SSHCraft ist
  öffentlich].

### Sprachen

- Englisch und Deutsch sind eingebaut; in den Einstellungen wählbar: **AUTO** (Vorgabe), Englisch,
  Deutsch.
- AUTO nimmt die Sprache des Betriebssystems; ist sie weder Deutsch noch Englisch, gilt Englisch
  [Claudes Entscheidung für den Rückfall].
- Alle Oberflächentexte und Fehlermeldungen laufen über Übersetzungsdateien; Fehler aus dem
  Rust-Kern kommen als Code an der Oberfläche an und werden dort übersetzt
  [Claudes Entscheidung].
- Weitere Sprachen lassen sich durch eine zusätzliche Übersetzungsdatei ergänzen.

## 3. Technik

[Claudes Entscheidung, Moritz hat den Vorschlag angenommen]

### Client: Tauri 2

- Eine Codebasis für alle Zielplattformen.

### Zielplattformen

Moritz, 12.11.2026 (ersetzt "Windows, Android und iOS"):

| Art | Plattform | Architekturen |
|---|---|---|
| Desktop | Windows | x64, x86, arm64 |
| Desktop | macOS | universal (Apple Silicon und Intel in einem Paket) |
| Desktop | Linux | x86_64, aarch64 |
| Desktop | FreeBSD | x86_64 |
| Mobil | Android | arm64, armv7, x86_64 (Emulator) |
| Mobil | iOS | arm64 |
| Mobil | iPadOS | arm64, eigenes Layout für große Bildschirme und Hardwaretastatur |

Was je Desktop-Plattform anders ist [Claudes Entscheidung, wo nicht anders vermerkt]:

- **Proton-Pass-Agent:** Windows über die Named Pipe, macOS und Linux über den Unix-Socket aus
  `SSH_AUTH_SOCK` (dort läuft jeder Agent, auch der von Proton Pass). Auf FreeBSD gibt es keine
  Proton-Pass-App, dort gilt nur der allgemeine Agent aus `SSH_AUTH_SOCK`.
- **Schlüsselspeicher für den Hauptschlüssel:** Windows Credential Manager, macOS Keychain,
  Linux und FreeBSD Secret Service (über `keyring`).
- **Prüfung vor gespeicherten Schlüsseln:** Windows Hello, auf macOS Touch ID oder das
  Kennwort des Benutzers. Linux und FreeBSD haben keine einheitliche Schnittstelle; dort
  verhält sich SSHCraft wie ein Windows ohne eingerichtetes Hello (Hinweis, keine Sperre).
- **Lokales Terminal:** Windows PowerShell, cmd oder WSL; sonst die Login-Shell des Benutzers.
- **FreeBSD:** Tauri unterstützt FreeBSD nicht offiziell. Gebaut wird in einer FreeBSD-VM in
  GitHub Actions gegen `webkit2-gtk` aus den Ports; ausgeliefert als Archiv mit Programm und
  `.desktop`-Datei, weil der Tauri-Bundler kein FreeBSD-Paket erzeugt.
- **Oberfläche:** React und TypeScript, Terminal mit xterm.js.
- **Kern in Rust:** SSH (`russh`), Verschlüsselung, Anbindung an den Proton-Pass-Agent über
  `russh::keys::agent::client::AgentClient::connect_named_pipe` (geprüft am 11.11.2026, die
  Funktion existiert im aktuellen `russh`), Aufruf von `pass-cli`, Sync-Client, lokaler Speicher.

### Server: Rust

- `axum`, Datenbank **SQLite als Standard, PostgreSQL optional**.
- Auslieferung als einzelnes Programm und als Docker-Image, damit andere es leicht betreiben.
- Ein gemeinsames Rust-Crate für Protokoll, Datentypen und Verschlüsselung, das Client und
  Server beide nutzen.

### Verteilung

Nicht über App Stores. Alle Apps und der Server werden aus den **GitHub Releases** geladen:
Windows-Installer (x64, x86, arm64), macOS-DMG (universal), Linux-Pakete (AppImage, `.deb`,
`.rpm`, je x86_64 und aarch64), FreeBSD-Archiv (x86_64), Android-APK, IPA für iOS und iPadOS
(siehe Abschnitt 13), Server-Programm für Linux, dazu das Docker-Image. Gebaut wird alles in GitHub Actions bei einem Release-Tag. Damit entfällt auch
der Konflikt zwischen AGPL und App-Store-Bedingungen.

### Repository

Ein Repository `musiker15/sshcraft`, lokal unter `G:\GitHub Repositories\Musiker15\sshcraft`.
Aufteilung in Unterordner (Client, Server, gemeinsames Crate) bleibt dem Builder überlassen.
Code, Kommentare und Bezeichner auf Englisch. README und Doku: Sprache dem Builder überlassen,
für ein internationales Publikum ist Englisch naheliegend.

## 4. Begriffe und Datenmodell

- **Ort:** Jeder Host und jede Gruppe gehört genau einem Ort: "nur auf diesem Gerät" oder einem
  bestimmten Sync-Server. Einträge lassen sich zwischen Orten verschieben. Die Seitenleiste zeigt
  alles zusammen, mit einer Markierung, woher ein Eintrag kommt.
- **Gruppe:** Sammlung von Hosts. Typisch ist eine Gruppe pro Server, darin mehrere Host-Einträge
  mit unterschiedlichen Benutzern und Schlüsseln (etwa `root`, `musiker15`, Deploy-User).
  Gruppen können auch thematisch genutzt werden ("Kunden", "Privat"). Einstellungen wie Adresse
  und Port, die an der Gruppe stehen, gelten für alle ihre Hosts, solange ein Host sie nicht
  selbst überschreibt.
- **Host:** Adresse, Port, Benutzer, zugeordneter Schlüssel, Anzeigename. Werte können von der
  Gruppe geerbt sein.
- **Schlüssel:** ein SSH-Schlüssel aus einer der drei Quellen (Abschnitt 5).
- **Konto:** ein Benutzer auf einem Sync-Server, mit Name, E-Mail und Kontoschlüssel.
- **Freigabe:** ein geteilter Host oder eine geteilte Gruppe (Abschnitt 8).

Gruppen können verschachtelt werden (Untergruppen, seit 12.11.2026 Pflicht, siehe Abschnitt 2).

## 5. Schlüsselquellen

Drei Wege, gleichwertig nebeneinander. Die dritte gibt es immer, damit SSHCraft auch ganz ohne
Proton Pass nutzbar ist.

| Quelle | Plattform | Wofür | Wie |
|---|---|---|---|
| Proton-Pass-Agent | Desktop | nur Verbinden | Client spricht die Agent-Pipe an. Der private Schlüssel verlässt Proton Pass nie, kann deshalb auch nicht synchronisiert werden. Im Host steht nur, welcher Agent-Schlüssel (per Fingerprint) gemeint ist. |
| Proton Pass über `pass-cli` | Desktop | Verbinden und Sync | Client listet die SSH-Schlüssel im Tresor über `pass-cli` auf, der Nutzer wählt, der Client liest den privaten Schlüssel einmal aus und speichert ihn verschlüsselt. Setzt ein installiertes und angemeldetes `pass-cli` voraus. |
| Eigener Schlüssel | überall | Verbinden und Sync | Import als Datei oder durch Einfügen; Erzeugen eines neuen Schlüssels im Client [Claudes Entscheidung]. |

Hinweis für den Builder: `pass-cli item view --output json` gibt private Schlüssel im Klartext aus.
Die Ausgabe darf nie geloggt oder in eine Datei geschrieben werden, nur im Speicher verarbeitet.

## 6. Konten, Anmeldung, Zugang zum Server

### Zugang zum Server

Beim ersten Start erzeugt der Server einen **API-Key**. Er gilt für alle Benutzer dieses Servers.
Wer sich verbinden will, gibt im Client ein: Serveradresse, API-Key, dazu den Benutzer. Der
Betreiber gibt den API-Key nur an die Leute weiter, die er auf dem Server haben will. Ohne
gültigen API-Key ist weder Registrieren noch Anmelden möglich.

Erzeugt der Betreiber den API-Key neu, bleiben bereits angemeldete Geräte angemeldet; nur neue
Geräte brauchen den neuen Key.

### Konto anlegen

Im Client: Name und E-Mail eingeben. Der **Client** erzeugt dabei das Schlüsselpaar des Kontos.
Der Server bekommt nur den öffentlichen Teil und speichert ihn zum Konto.

Moritz hatte ursprünglich "der Server erzeugt den Schlüssel" beschrieben. Weil der Server die
SSH-Schlüssel nie lesen darf, erzeugt ihn stattdessen der Client [Claudes Entscheidung, mit
Moritz besprochen]. Aus Sicht des Nutzers ändert sich nichts.

Nach dem Anlegen zeigt der Client den Kontoschlüssel zum Sichern an, mit dem klaren Hinweis, ihn
in Proton Pass (oder einem anderen Passwortmanager) abzulegen. Wer ihn verliert, verliert den
Zugang und alle verschlüsselten Daten; es gibt keine Wiederherstellung durch den Server, und das
ist so gewollt.

### Anmelden

Ausschließlich mit dem Kontoschlüssel, kein Passwort. Ablauf: Server schickt eine Zufalls-Challenge,
Client signiert sie mit dem Kontoschlüssel, Server prüft gegen den gespeicherten öffentlichen
Schlüssel und gibt ein Sitzungs-Token aus.

Gibt es das Konto schon (neues Gerät), wird **kein** neuer Schlüssel erzeugt. Der Nutzer bringt
den vorhandenen Kontoschlüssel auf das Gerät:

- durch Einfügen, etwa aus Proton Pass kopiert, oder
- durch Scannen eines QR-Codes, den ein bereits angemeldeter Client anzeigt.

[Beide Wege: Claudes Entscheidung, weil offen gelassen.]

### Geräte und Geräte-Sperre

Ein verlorenes oder gestohlenes Gerät muss sich einzeln sperren lassen, ohne die anderen Geräte
zu stören. Der Benutzer kann das selbst von jedem anderen seiner Geräte aus tun.

Daraus folgt das Modell:

- Der **Kontoschlüssel** ist die Wurzel des Kontos. Er liegt beim Nutzer (Proton Pass oder ein
  anderer Passwortmanager), nicht dauerhaft auf den Geräten.
- Beim Einrichten (Einfügen oder QR-Code) erzeugt das Gerät einen eigenen **Geräteschlüssel**.
  Der Kontoschlüssel unterschreibt ihn, danach verwirft das Gerät den Kontoschlüssel wieder.
  Anmelden und Entschlüsseln laufen im Alltag über den Geräteschlüssel. Die Verschlüsselung für
  Freigaben richtet sich an die Geräte eines Kontos [Aufbau im Einzelnen dem Builder überlassen].
- Jedes angemeldete Gerät sieht die Liste der Geräte des Kontos (Name, Plattform, zuletzt aktiv)
  und kann jedes andere sperren. Ein gesperrtes Gerät wird vom Server abgewiesen und bekommt
  keine Daten mehr.
- Beim Sperren erzeugt der sperrende Client neue Schlüssel für alles, was das gesperrte Gerät
  entschlüsseln konnte, und verteilt sie an die übrigen Geräte und Freigabe-Empfänger, damit auch
  ein übernommener Server dem gesperrten Gerät nichts Neues zuspielen kann.
- Wie beim Entziehen einer Freigabe gilt: SSH-Schlüssel, die schon auf dem gesperrten Gerät lagen,
  können kopiert worden sein. Der Client weist darauf hin und bietet den Tausch an.
- Ist kein Gerät mehr übrig, richtet man mit dem Kontoschlüssel ein neues ein und sperrt von dort
  die alten.
- Der Server-Betreiber kann Geräte eines Benutzers per Befehl auflisten und sperren
  [Claudes Entscheidung].

### E-Mail

Die E-Mail ist innerhalb eines Servers eindeutig und dient dazu, Empfänger beim Teilen zu finden.
Der Server verschickt keine E-Mails und braucht keinen Mailversand [Claudes Entscheidung].

## 7. Sync

### Einstellbar pro Server im Client

- Sync aus oder an.
- Was synchronisiert wird: **nur Hosts und Gruppen**, oder **Hosts, Gruppen und Schlüssel**.

Ist der Schlüssel-Sync aus, bleiben Schlüssel auf dem Gerät; ein synchronisierter Host zeigt dann
auf anderen Geräten an, dass dort noch ein Schlüssel zugeordnet werden muss
[Claudes Entscheidung].

### Mehrere Server

Der Client kann mehrere Sync-Server gleichzeitig verwalten. Auf jedem Server hat man ein eigenes
Konto mit eigenem Kontoschlüssel.

### Ende-zu-Ende-Verschlüsselung

Harte Anforderung: Der Server kann keine Host-Daten und keine Schlüssel lesen, auch nicht nach
einem Einbruch.

- Alles, was ein Nutzer synchronisiert, wird auf dem Client verschlüsselt, bevor es den Client
  verlässt. Der Server speichert verschlüsselte Datenpakete plus die Metadaten, die er zum
  Verwalten braucht (Besitzer, Freigaben, Versionsstand).
- Jede Gruppe und der persönliche Bereich jedes Nutzers haben einen eigenen symmetrischen
  Schlüssel. Dieser wird für jeden berechtigten Nutzer mit dessen öffentlichem Schlüssel
  verschlüsselt abgelegt.
- Der Kontoschlüssel braucht dafür einen Teil zum Signieren (Anmeldung) und einen zum
  Verschlüsseln (Empfang von Gruppenschlüsseln). Konkrete Verfahren wählt der Builder, mit
  etablierten Bibliotheken und ohne eigene Kryptografie.

Ob auch Namen von Hosts und Gruppen verschlüsselt sind: ja, alles außer den Metadaten, die der
Server zwingend braucht [Claudes Entscheidung].

### Angreifermodell: auch ein übernommener Server

[Claudes Entscheidung, abgeleitet aus "der Server darf die Schlüssel nie lesen, auch wenn er
gehackt wird"] Wer den Server übernimmt, kann nicht nur die Datenbank lesen, sondern auch
verändern, was der Server ausliefert. SSHCraft schützt deshalb auch dagegen:

- **Öffentliche Schlüssel anderer Nutzer prüfbar machen.** Beim Teilen zeigt der Client den
  Fingerprint des Empfängerschlüssels an, und beide Seiten können ihn vergleichen (wie die
  Sicherheitsnummer bei Signal). Einmal gesehene Schlüssel merkt sich der Client; ändert sich der
  Schlüssel eines Kontakts, warnt er deutlich, bevor er etwas für ihn verschlüsselt.
- **Datensätze signieren.** Jeder synchronisierte Eintrag ist vom schreibenden Konto signiert, der
  Client verwirft Einträge mit ungültiger Signatur oder von Konten ohne Schreibrecht. So kann der
  Server keine Host-Adresse und keinen Host-Fingerprint unterschieben (Gefahr eines
  SSH-Man-in-the-Middle).
- **Rückfall auf alte Stände erkennen.** Einträge tragen eine Versionsnummer; der Client akzeptiert
  keinen älteren Stand als den, den er schon kennt.
- Die E-Mail wird nicht bestätigt, sie ist nur ein Suchbegriff. Vertrauen entsteht über den
  Fingerprint-Vergleich, nicht über die E-Mail.

### Live und offline

- Änderungen kommen sofort bei allen Geräten und allen Empfängern an (Push vom Server, etwa per
  WebSocket; Mechanismus dem Builder überlassen).
- Der Client funktioniert ohne Verbindung zum Server weiter, mit dem zuletzt bekannten Stand, und
  gleicht nach. Wie Konflikte bei gleichzeitigen Änderungen gelöst werden, entscheidet der Builder;
  für die erste Version reicht "die letzte Änderung gewinnt" pro Eintrag
  [Claudes Entscheidung].

## 8. Teilen

- Teilen lässt sich **ein einzelner Host** oder **eine Gruppe**.
- Bei einer Gruppe: **alle Hosts** oder **eine Auswahl**. Bei "alle Hosts" gehören später
  hinzugefügte Hosts automatisch dazu, bei einer Auswahl nicht.
- Pro Freigabe: **mit oder ohne SSH-Schlüssel**.
- Pro Freigabe: **lesen oder schreiben**. Mit Schreibrecht darf der Empfänger Hosts ändern,
  hinzufügen und entfernen; alle Beteiligten bekommen das live.
- Empfänger: andere Benutzer auf demselben Server, ausgewählt per E-Mail.
- Ohne Schlüssel geteilte Hosts zeigen beim Empfänger an, dass er einen eigenen Schlüssel
  zuordnen muss.
- Ein Schlüssel, der aus dem Proton-Pass-Agent kommt, kann nicht mitgeteilt werden (der Client
  hat ihn nie). Der Client sagt das beim Teilen.

**Wie Freigaben technisch und rechtlich funktionieren** [Claudes Entscheidungen]:

- Jede Freigabe ist ein eigener Bereich mit eigenem symmetrischem Schlüssel. Wird ein einzelner
  Host oder eine Auswahl aus einer Gruppe geteilt, liegt in diesem Bereich eine Kopie der
  betroffenen Hosts; der Client des Besitzers hält Original und Kopie gleich. Wird eine Gruppe
  mit "alle Hosts" geteilt, ist der Gruppenbereich selbst der geteilte Bereich.
- Eigentümer einer geteilten Gruppe bleibt, wer sie geteilt hat. Hosts, die ein Empfänger mit
  Schreibrecht hinzufügt, gehören zur Gruppe und damit dem Eigentümer.
- Bei einer Auswahl oder einem einzelnen Host bedeutet Schreibrecht: die geteilten Hosts ändern.
  Hinzufügen und Entfernen von Hosts gibt es nur bei einer Gruppe, die mit "alle Hosts" geteilt ist.
- Empfänger dürfen nicht weiterteilen und die Gruppe nicht löschen, auch mit Schreibrecht nicht.
  Das darf nur der Eigentümer.
- Beim Entziehen erzeugt der Client des Eigentümers einen neuen Bereichsschlüssel, verschlüsselt
  die Inhalte neu und verteilt den neuen Schlüssel an die verbliebenen Empfänger. Die anderen
  Empfänger müssen dafür nichts tun.

**Freigabe entziehen:** Der Empfänger verliert den Zugriff auf künftige Stände. Hatte er den
Schlüssel, kann er ihn kopiert haben. Der Client weist beim Entziehen darauf hin und bietet an,
den Schlüssel zu tauschen; außerdem wird der Gruppenschlüssel neu erzeugt, damit der Empfänger
künftige Änderungen nicht mehr entschlüsseln kann.

## 9. Terminal und Bedienung

- Mehrere Sitzungen gleichzeitig (Tabs auf dem Desktop, Liste oder Wischen auf dem Handy;
  Gestaltung dem Builder überlassen).
- **Host-Schlüssel des Servers:** Beim ersten Verbinden zeigt der Client den Fingerprint und
  fragt; danach wird er gespeichert und bei Abweichung deutlich gewarnt. Bekannte Fingerprints
  werden mit dem Host synchronisiert und geteilt [Claudes Entscheidung].
- **Keep-Alive und automatisches Wiederverbinden** nach Netzwechsel oder Abbruch
  [Claudes Entscheidung].
- **Sitzungen im Hintergrund:** Auf Android laufen offene Sitzungen über einen
  Vordergrund-Dienst (mit Benachrichtigung) weiter. Auf iOS lässt das System das nicht dauerhaft
  zu; dort verbindet SSHCraft beim Zurückkehren neu und sagt das deutlich an
  [Claudes Entscheidung].
- **Import aus `~/.ssh/config`** auf dem Desktop, damit vorhandene Hosts nicht von Hand neu
  angelegt werden müssen [Claudes Entscheidung]. Einen Import aus Termius gibt es nicht, weil
  Termius dafür keine brauchbare Ausfuhr bietet.
- **Zusatzleiste auf dem Handy** über der Tastatur, wie bei Termius: Strg, Esc, Tab, Alt,
  Pfeiltasten und schwer erreichbare Zeichen (`|`, `~`, `/`, `-`). Strg und Alt wirken als
  Umschalter für die nächste Taste.

## 10. Anbindung für KI-Agents (Claude Code, Codex und andere)

Agents sollen über SSHCraft alles tun können, was im Client möglich ist, aber unter keinen
Umständen einen privaten Schlüssel zu sehen bekommen.

### Wie Agents andocken

- Der Desktop-Client bringt einen **MCP-Server** mit (`sshcraft mcp`, Transport stdio), den
  Claude Code, Codex und andere MCP-fähige Agents einbinden. Dazu eine **Kommandozeile**
  `sshcraft` mit denselben Aktionen für Agents und Skripte ohne MCP [Claudes Entscheidung].
- Beide sind nur dünne Vermittler: Sie sprechen über eine lokale, nur für den eigenen
  Windows-Benutzer zugängliche Verbindung mit dem **laufenden Desktop-Client**. Der Client hält
  die Schlüssel, baut die SSH-Verbindungen auf und prüft die Rechte. Läuft er nicht, bekommt der
  Agent eine klare Fehlermeldung [Claudes Entscheidung].
- Nur Desktop, nicht auf dem Handy.

### Keine privaten Schlüssel für Agents

- Keine Aktion gibt Schlüsselmaterial heraus, es gibt auch keine Export-Aktion für Agents. Ein
  Agent sieht von einem Schlüssel höchstens Namen, Typ und Fingerprint.
- SSH-Verbindungen baut immer der Rust-Kern auf. Der Agent schickt Befehle und bekommt Ausgabe
  zurück (einmalige Befehle und eine länger offene Sitzung, in die er schreibt und aus der er
  liest; Schnitt dem Builder überlassen).
- Auch der Kontoschlüssel und Geräteschlüssel sind für Agents unerreichbar.
- **Grenze, die SSHCraft nicht schließen kann:** Ein Agent mit eigener Shell läuft als derselbe
  Windows-Benutzer und könnte an SSHCraft vorbei `pass-cli` aufrufen. SSHCraft erschwert den Griff
  auf den eigenen Speicher: Die lokal gespeicherten Schlüssel werden nach dem Start erst nach
  Windows Hello (Fingerabdruck, Gesicht oder PIN) entsperrt [Claudes Entscheidung]. Die Doku
  erklärt die Grenze offen.

### Rechte für Agents

In den Einstellungen des Clients steht **jede Aktion**, die der Client kennt, mit drei Stufen:

- **Erlaubt:** der Agent darf ohne Rückfrage.
- **Fragen:** der Client zeigt vor der Ausführung ein eigenes Bestätigungsfenster mit Agent,
  Aktion und Ziel (etwa Host und Befehl). Die Bestätigung passiert ausschließlich in der
  Oberfläche von SSHCraft, nie im Chat des Agents, damit ein Agent sie nicht selbst geben kann.
- **Gesperrt:** der Agent bekommt eine Ablehnung.

Vorgaben für eine frische Installation [Claudes Entscheidung]:

| Aktion | Vorgabe |
|---|---|
| Hosts, Gruppen, Schlüsselnamen auflisten und ansehen | Erlaubt |
| Verbinden, Befehl ausführen | Fragen |
| Hosts und Gruppen anlegen, ändern | Fragen |
| Hosts, Gruppen, Schlüssel löschen | Gesperrt |
| Schlüssel importieren oder erzeugen | Fragen |
| Teilen, Freigaben ändern oder entziehen | Gesperrt |
| Geräte sperren, Sync-Server und Konten verwalten | Gesperrt |
| Einstellungen ändern, auch die Rechte für Agents | gibt es für Agents nicht |

Zusätzlich lassen sich einzelne Gruppen oder Hosts **für Agents ausblenden**; sie tauchen in
keiner Agent-Aktion auf, etwa für Produktivserver [Claudes Entscheidung].

Jede Agent-Aktion landet in einem **Protokoll** im Client (Zeit, Aktion, Ziel, erlaubt oder
abgelehnt, bei Befehlen der Befehl) [Claudes Entscheidung].

## 11. Sicherheit auf dem Gerät

- **App-Sperre (Handy):** in den Einstellungen wählbar: aus, PIN, Fingerabdruck, Face ID.
  Auf dem Desktop keine eigene Sperre in der ersten Version [Claudes Entscheidung].
- **Lokaler Speicher:** Private Schlüssel, Kontoschlüssel und Sitzungs-Token liegen nie im
  Klartext auf der Platte. Geschützt über den Schlüsselspeicher des Betriebssystems (Windows
  Credential Manager bzw. DPAPI, macOS Keychain, Secret Service unter Linux und FreeBSD,
  Android Keystore, iOS und iPadOS Keychain); Umsetzung dem Builder überlassen.
- Nichts Geheimes in Logs.

## 12. Server-Verwaltung

Nur per Befehl auf dem Server, keine Web-Oberfläche. Mindestens:

- Benutzer auflisten
- Benutzer sperren und entsperren
- Benutzer löschen
- Geräte eines Benutzers auflisten und sperren
- API-Key anzeigen und neu erzeugen

**Wird ein Benutzer gesperrt oder gelöscht, der Eigentümer geteilter Gruppen ist**
[Claudes Entscheidung]: Beim Sperren bleiben seine Freigaben für die Empfänger lesbar, ändern
kann sie niemand mehr. Beim Löschen geht die Eigentümerschaft auf einen Empfänger mit
Schreibrecht über (der Server ändert nur die Zuordnung, den Schlüssel hat der Empfänger schon);
gibt es keinen, verschwindet die Freigabe bei allen Empfängern. Der Löschbefehl zeigt vorher an,
welche Freigaben betroffen sind.

Betrieb hinter einem Reverse Proxy (Apache, Nginx, Caddy) mit TLS ist der erwartete Fall; der
Server selbst spricht HTTP auf einem einstellbaren Port. Die Doku zeigt mindestens ein
Apache-Beispiel [Claudes Entscheidung].

## 13. Risiken und offene Prüfpunkte

- **Tastatur auf dem Handy mit xterm.js im Webview:** muss früh mit einem Prototyp auf Android
  geprüft werden (Eingabe, Autokorrektur aus, Zusatzleiste). Fällt das schlecht aus, braucht der
  Terminal-Teil auf Mobilgeräten eine andere Lösung.
- **Tauri 2 auf Mobilgeräten** ist jünger als auf dem Desktop; Biometrie und Keychain über
  Plugins früh anbinden und prüfen.
- **iOS ohne App Store:** Apple lässt Apps außerhalb des App Stores nur eingeschränkt zu. Ein
  IPA aus den GitHub Releases lässt sich nur per Sideloading installieren (etwa AltStore oder
  Sideloadly, mit kostenlosem Apple-Konto alle 7 Tage neu signiert) oder selbst in Xcode bauen.
  Gebaut wird das IPA auf einem macOS-Runner in GitHub Actions. Deshalb kommt iOS im Bauplan
  nach Windows und Android [Claudes Entscheidung].
- **Windows ohne Codesignatur** löst beim ersten Start die SmartScreen-Warnung aus; ob ein
  Signaturzertifikat angeschafft wird, ist für die erste Version nicht nötig.
- **macOS ohne Notarisierung** wird von Gatekeeper beim ersten Start blockiert (Rechtsklick,
  Öffnen). Ein Apple-Entwicklerkonto ist für die erste Version nicht vorgesehen.
- **FreeBSD** ist für Tauri keine offizielle Plattform; Bau und Test in einer VM können
  an Abhängigkeiten scheitern. Fällt das schlecht aus, kommt FreeBSD nach Version 1.
- **Windows x86 und Windows arm64** werden in der CI gebaut, aber mangels Gerät nur durch
  die Tests auf dem Runner geprüft.
- **`pass-cli`** muss installiert und angemeldet sein; der Client erkennt, wenn nicht, und erklärt
  den Weg.

## 14. Dem Builder überlassen

- Aufteilung des Repositorys und Wahl der Unterbibliotheken.
- Konkrete Krypto-Verfahren (mit etablierten Bibliotheken).
- Push-Mechanismus, Konfliktauflösung über "letzte Änderung gewinnt" hinaus.
- Gestaltung der Oberfläche im Einzelnen, im Rahmen von "Aussehen" in Abschnitt 2.
- Sprache von README und Doku.
