# Stollen: Fahrplan der Teilpläne

Design: `docs/superpowers/specs/2026-11-11-stollen-design.md` (freigegeben am 11.11.2026).

Das Design umfasst mehrere voneinander trennbare Teilsysteme. Jeder Teilplan unten ergibt für
sich lauffähige, getestete Software. Ausführlich geschrieben wird immer nur der nächste Teilplan,
mit dem Wissen aus den vorherigen.

| Nr. | Teilplan | Ergebnis | Design-Abschnitte |
|---|---|---|---|
| 1 | Desktop-Grundlage | Windows-App: Gruppen und Hosts anlegen, eigene Schlüssel importieren oder erzeugen (verschlüsselt gespeichert), Terminal in Tabs, Host-Fingerprint-Prüfung, Keep-Alive, Wiederverbinden | 3, 4, 5 (eigener Schlüssel), 9, 11 |
| 2 | Android-Spike (Wegwerf) | Antwort auf das Risiko "Tastatur mit xterm.js im Webview": Tauri-2-App auf Android, Terminal, Zusatzleiste als Rohbau, Urteil gut genug oder nicht | 13 |
| 3 | Schlüsselquellen und Windows-Anbindung | Proton-Pass-Agent über die Named Pipe, Import über `pass-cli`, Import aus `~/.ssh/config`, Entsperren mit Windows Hello | 5, 9, 10 (Windows Hello) |
| 4 | Protokoll- und Krypto-Crate | `stollen-proto`: Konto- und Geräteschlüssel, Signaturen, Bereichsschlüssel, verschlüsselte und signierte Datensätze mit Versionen, Prüfung gegen Rückfall | 6, 7 |
| 5 | Sync-Server | `stollen-server`: axum, SQLite und PostgreSQL, API-Key, Challenge-Anmeldung, Konten, Geräte, Datensätze, Push per WebSocket, Verwaltungsbefehle, Docker-Image, Apache-Beispiel | 6, 7, 12 |
| 6 | Client-Sync | Mehrere Server, Konto anlegen, Gerät einrichten (Einfügen, QR), Sync-Einstellungen, Live-Abgleich, Offline-Betrieb, Geräteliste und Geräte-Sperre mit Schlüsseltausch | 4 (Orte), 6, 7 |
| 7 | Teilen | Freigaben für Host und Gruppe, Auswahl, mit oder ohne Schlüssel, lesen oder schreiben, Fingerprint-Vergleich, Entziehen mit Schlüsseltausch, Verhalten bei gesperrtem oder gelöschtem Eigentümer | 8, 12 |
| 8 | Agent-Anbindung | Lokale Verbindung zum Client, `stollen mcp`, Kommandozeile `stollen`, Rechte pro Aktion, Bestätigungsfenster, Ausblenden, Protokoll | 10 |
| 9 | Android vollständig und Releases | Android-Keystore, Zusatzleiste, Vordergrund-Dienst, App-Sperre; GitHub-Actions-Releases für Windows, Android, Server und Docker | 3 (Verteilung), 9, 11 |
| 10 | iOS | Keychain, Face ID, Wiederverbinden beim Zurückkehren, IPA-Bau auf macOS-Runner, Anleitung zum Sideloading | 9, 11, 13 |

Reihenfolge: 1 und 2 zuerst, weil 2 das größte technische Risiko klärt, bevor viel Oberfläche
gebaut ist. Fällt der Spike schlecht aus, wird vor Teilplan 9 entschieden, wie das Terminal auf
dem Handy anders gelöst wird.

Nicht in einem Teilplan, weil im Design als "später" geführt: Portweiterleitung, Sprung-Hosts,
Agent-Weiterleitung, Mosh, Snippets, Sitzungsaufzeichnung. Ebenfalls nicht geplant, weil im
Design nicht vorgesehen: Anmeldung am SSH-Server per Passwort (Stollen arbeitet nur mit
Schlüsseln; nachrüstbar, falls gewünscht).
