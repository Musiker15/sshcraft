# Stollen: Fahrplan der Teilpläne

Design: `docs/superpowers/specs/2026-11-11-stollen-design.md` (freigegeben am 11.11.2026).

Das Design umfasst mehrere voneinander trennbare Teilsysteme. Jeder Teilplan unten ergibt für
sich lauffähige, getestete Software. Ausführlich geschrieben wird immer nur der nächste Teilplan,
mit dem Wissen aus den vorherigen.

| Nr. | Teilplan | Ergebnis | Design-Abschnitte | Stand |
|---|---|---|---|---|
| 1 | Desktop-Grundlage | Windows-App: Gruppen und Hosts anlegen, eigene Schlüssel importieren oder erzeugen (verschlüsselt gespeichert), Terminal in Tabs, Host-Fingerprint-Prüfung, Keep-Alive, Wiederverbinden | 3, 4, 5 (eigener Schlüssel), 9, 11 | fertig, auf `main` |
| 2 | Schlüsselquellen und Windows-Anbindung | Proton-Pass-Agent über die Named Pipe, Import über `pass-cli`, Import aus `~/.ssh/config`, Entsperren mit Windows Hello | 5, 9, 10 (Windows Hello) | offen |
| 3 | Protokoll- und Krypto-Crate | `stollen-proto`: Konto- und Geräteschlüssel, Signaturen, Bereichsschlüssel, verschlüsselte und signierte Datensätze mit Versionen, Prüfung gegen Rückfall | 6, 7 | offen |
| 4 | Sync-Server | `stollen-server`: axum, SQLite und PostgreSQL, API-Key, Challenge-Anmeldung, Konten, Geräte, Datensätze, Push per WebSocket, Verwaltungsbefehle, Docker-Image, Apache-Beispiel | 6, 7, 12 | offen |
| 5 | Client-Sync | Mehrere Server, Konto anlegen, Gerät einrichten (Einfügen, QR), Sync-Einstellungen, Live-Abgleich, Offline-Betrieb, Geräteliste und Geräte-Sperre mit Schlüsseltausch | 4 (Orte), 6, 7 | offen |
| 6 | Teilen | Freigaben für Host und Gruppe, Auswahl, mit oder ohne Schlüssel, lesen oder schreiben, Fingerprint-Vergleich, Entziehen mit Schlüsseltausch, Verhalten bei gesperrtem oder gelöschtem Eigentümer | 8, 12 | offen |
| 7 | Agent-Anbindung | Lokale Verbindung zum Client, `stollen mcp`, Kommandozeile `stollen`, Rechte pro Aktion, Bestätigungsfenster, Ausblenden, Protokoll | 10 | offen |
| 8 | Releases für Windows und Server | GitHub-Actions-Releases: Windows-Installer, Server-Programm für Linux, Docker-Image | 3 (Verteilung) | offen |
| 9 | Android-Spike (Wegwerf) | Antwort auf das Risiko "Tastatur mit xterm.js im Webview": Tauri-2-App auf Android, Terminal, Zusatzleiste als Rohbau, Urteil gut genug oder nicht | 13 | angefangen, Zweig `spike/android` |
| 10 | Android vollständig | Android-Keystore, Zusatzleiste, Vordergrund-Dienst, App-Sperre, APK in den Releases | 3 (Verteilung), 9, 11 | offen |
| 11 | iOS | Keychain, Face ID, Wiederverbinden beim Zurückkehren, IPA-Bau auf macOS-Runner, Anleitung zum Sideloading | 9, 11, 13 | offen |

Reihenfolge (Moritz, 12.11.2026): erst Windows und Server, also 2 bis 8, danach Android und iOS.
Bewusst in Kauf genommen: Das Risiko der Handytastatur wird erst mit Teilplan 9 geklärt. Weil die
Oberfläche fürs Handy ohnehin eigene Bedienelemente braucht, ist der Schaden klein, falls das
Terminal dort anders gelöst werden muss. Die Android-Werkzeuge (SDK, NDK 30.0.16248370, Rust-Ziele)
sind schon installiert, `ANDROID_HOME` ist `G:\Android\Sdk`.

Der Zweig `spike/android` enthält einen Tresor-Ersatz für Android, der den Hauptschlüssel als
Klartext-Datei ablegt. Er ist nur für den Spike gedacht und darf so nie nach `main`.

Nicht in einem Teilplan, weil im Design als "später" geführt: Portweiterleitung, Sprung-Hosts,
Agent-Weiterleitung, Mosh, Snippets, Sitzungsaufzeichnung. Ebenfalls nicht geplant, weil im
Design nicht vorgesehen: Anmeldung am SSH-Server per Passwort (Stollen arbeitet nur mit
Schlüsseln; nachrüstbar, falls gewünscht).
