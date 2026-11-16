//! German. One arm per English text; `tests/i18n.rs` fails when a text of the interface has none.

pub fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Close" => "Schließen",
        "Hosts" => "Hosts",
        "Keys" => "Schlüssel",
        "Settings" => "Einstellungen",
        "Terminal" => "Terminal",
        "{count} hosts" => "{count} Hosts",
        "Open the command palette" => "Befehlspalette öffnen",
        "Command palette" => "Befehlspalette",
        "Type a command" => "Befehl eingeben",
        "No command matches" => "Kein Befehl passt",
        "New tab" => "Neuer Tab",
        "Close the current tab" => "Aktuellen Tab schließen",
        "Next tab" => "Nächster Tab",
        "Previous tab" => "Vorheriger Tab",
        "Copy the selection" => "Auswahl kopieren",
        "Paste" => "Einfügen",
        "Close tab" => "Tab schließen",
        "Local terminal" => "Lokales Terminal",
        "Serial port" => "Serieller Port",
        "Search hosts" => "Hosts suchen",
        // Hosts and groups
        "A group needs a name." => "Eine Gruppe braucht einen Namen.",
        "A host needs a name." => "Ein Host braucht einen Namen.",
        "Add variable" => "Variable hinzufügen",
        "Address" => "Adresse",
        "Agent forwarding" => "Agent-Weiterleitung",
        "Agent key {fingerprint}" => "Agent-Schlüssel {fingerprint}",
        "All hosts" => "Alle Hosts",
        "Cancel" => "Abbrechen",
        "comma separated" => "durch Kommas getrennt",
        "Connect" => "Verbinden",
        "Delete" => "Löschen",
        "Delete the group {name} with its subgroups and all their hosts?" => {
            "Die Gruppe {name} mit ihren Untergruppen und allen ihren Hosts löschen?"
        }
        "Delete the host {name}?" => "Den Host {name} löschen?",
        "Direct" => "Direkt",
        "Duplicate" => "Duplizieren",
        "Edit" => "Bearbeiten",
        "Edit group" => "Gruppe bearbeiten",
        "Edit host" => "Host bearbeiten",
        "Environment" => "Umgebung",
        "Forget host key" => "Host-Schlüssel vergessen",
        "Grid" => "Raster",
        "Group" => "Gruppe",
        "Identity" => "Identität",
        "Inherit" => "Erben",
        "Inherit ({state}, from {group})" => "Erben ({state}, von {group})",
        "Inherit or none" => "Erben oder keiner",
        "Inside" => "In",
        "Jump hosts" => "Sprung-Hosts",
        "Key" => "Schlüssel",
        "List" => "Liste",
        "Move to" => "Verschieben nach",
        "Name" => "Name",
        "New group" => "Neue Gruppe",
        "New host" => "Neuer Host",
        "No hosts here yet." => "Hier gibt es noch keine Hosts.",
        "None" => "Keiner",
        "None stored" => "Keines gespeichert",
        "Not set" => "Nicht gesetzt",
        "Off" => "Aus",
        "off" => "aus",
        "On" => "An",
        "on" => "an",
        "Open" => "Öffnen",
        "Password" => "Passwort",
        "Port" => "Port",
        "Proxy" => "Proxy",
        "Proxy address" => "Proxy-Adresse",
        "Run a program to connect?" => "Ein Programm zum Verbinden starten?",
        "{host} connects through this program on your computer. It was not set up on this device (it came with sync, a share or an agent, or it changed). Allow it only if you know it." => {
            "{host} verbindet sich über dieses Programm auf deinem Rechner. Es wurde nicht auf diesem Gerät eingerichtet (es kam über Sync, eine Freigabe oder einen Agent, oder es hat sich geändert). Erlaube es nur, wenn du es kennst."
        }
        "Allow on this device" => "Auf diesem Gerät erlauben",
        "Known hosts" => "Bekannte Server",
        "From a password manager" => "Aus einem Passwortmanager",
        "One-time codes" => "Einmalcodes",
        "TOTP secret or otpauth:// link" => "TOTP-Geheimnis oder otpauth://-Link",
        "This is not a TOTP secret (Base32 or an otpauth:// link)." => "Das ist kein TOTP-Geheimnis (Base32 oder ein otpauth://-Link).",
        "Reference" => "Verweis",
        "A {manager} reference looks like {example}" => "Ein Verweis für {manager} sieht so aus: {example}",
        "The reference holds a control character or starts with '-'" => "Der Verweis enthält ein Steuerzeichen oder beginnt mit '-'",
        "Fetched with {program} when connecting; SSHCraft keeps no copy." => "Wird beim Verbinden mit {program} geholt; SSHCraft behält keine Kopie.",
        "Send a password from your password manager?" => "Ein Passwort aus deinem Passwortmanager senden?",
        "{host} wants this password from your password manager, sent to the server below. It was not set up for this server on this device (it came with sync or a share, or the address changed). Allow it only if the password belongs to this server." => {
            "{host} möchte dieses Passwort aus deinem Passwortmanager an den Server unten senden. Es wurde auf diesem Gerät nicht für diesen Server eingerichtet (es kam über Sync oder eine Freigabe, oder die Adresse hat sich geändert). Erlaube es nur, wenn das Passwort zu diesem Server gehört."
        }
        "Trash" => "Papierkorb",
        "Connections" => "Verbindungen",
        "Every connection SSHCraft makes by itself is listed under Settings › Connections." => {
            "Jede Verbindung, die SSHCraft selbst aufbaut, steht unter Einstellungen › Verbindungen."
        }
        "Everything SSHCraft connects to or starts by itself. Your own SSH connections, port forwards and proxy commands are not listed." => {
            "Alles, womit SSHCraft sich selbst verbindet oder was es selbst startet. Deine eigenen SSH-Verbindungen, Portweiterleitungen und Proxy-Befehle stehen nicht darin."
        }
        "Sync with {name}: HTTPS, and a WebSocket for changes" => "Sync mit {name}: HTTPS und ein WebSocket für Änderungen",
        "{name}: passwords of {count} hosts or groups, fetched when connecting" => {
            "{name}: Passwörter von {count} Hosts oder Gruppen, beim Verbinden geholt"
        }
        "Keys from the SSH agent (Proton Pass), when a host uses one" => "Schlüssel aus dem SSH-Agent (Proton Pass), wenn ein Host einen nutzt",
        "AI agents connect here, from this computer only" => "KI-Agents verbinden sich hier, nur von diesem Rechner",
        "SSHCraft does not check for updates and sends no telemetry" => "SSHCraft sucht nicht nach Updates und sendet keine Telemetrie",
        "App lock" => "App-Sperre",
        "Lock after" => "Sperren nach",
        " minutes without input" => " Minuten ohne Eingabe",
        "0 never locks. Unlocking asks Windows Hello, and keys and passwords ask it again." => {
            "0 sperrt nie. Zum Entsperren fragt Windows Hello, und Schlüssel und Passwörter fragen es erneut."
        }
        "Not on this system yet: unlocking needs Windows Hello." => "Auf diesem System noch nicht: zum Entsperren braucht es Windows Hello.",
        "SSHCraft is locked" => "SSHCraft ist gesperrt",
        "Sessions keep running. Unlock with Windows Hello to see them." => {
            "Die Sitzungen laufen weiter. Entsperre mit Windows Hello, um sie zu sehen."
        }
        "Unlock" => "Entsperren",
        "Backup" => "Sicherung",
        "One file with your hosts, groups, identities, keys, passwords, snippets and host keys, encrypted with a passphrase. Restoring adds what is missing and changes nothing that is here." => {
            "Eine Datei mit deinen Hosts, Gruppen, Identitäten, Schlüsseln, Passwörtern, Snippets und Hostschlüsseln, verschlüsselt mit einer Passphrase. Beim Wiederherstellen kommt hinzu, was fehlt; was hier ist, bleibt unverändert."
        }
        "Create backup…" => "Sicherung erstellen …",
        "Restore from backup…" => "Aus Sicherung wiederherstellen …",
        "Restore from backup" => "Aus Sicherung wiederherstellen",
        "Create backup" => "Sicherung erstellen",
        "Passphrase again" => "Passphrase wiederholen",
        "Keep it in your password manager: without it the file cannot be opened." => {
            "Bewahre sie im Passwortmanager auf: ohne sie lässt sich die Datei nicht öffnen."
        }
        "Choose file…" => "Datei wählen …",
        "Save as…" => "Speichern unter …",
        "The passphrase needs at least {count} characters." => "Die Passphrase braucht mindestens {count} Zeichen.",
        "The two passphrases differ." => "Die beiden Passphrasen sind verschieden.",
        "Restored: {hosts} hosts, {groups} groups, {keys} keys, {passwords} passwords, {snippets} snippets." => {
            "Wiederhergestellt: {hosts} Hosts, {groups} Gruppen, {keys} Schlüssel, {passwords} Passwörter, {snippets} Snippets."
        }
        "The backup is written." => "Die Sicherung ist geschrieben.",
        "Snippet" => "Snippet",
        "Deleted hosts, groups, keys and snippets stay here for {days} days, also when another device deleted them." => {
            "Gelöschte Hosts, Gruppen, Schlüssel und Snippets bleiben {days} Tage hier, auch wenn ein anderes Gerät sie gelöscht hat."
        }
        "Keep for" => "Aufbewahren",
        " days" => " Tage",
        "The trash is empty." => "Der Papierkorb ist leer.",
        "Delete for good" => "Endgültig löschen",
        "Empty the trash" => "Papierkorb leeren",
        "Earlier versions…" => "Frühere Fassungen …",
        "Earlier versions of {name}" => "Frühere Fassungen von {name}",
        "No earlier versions." => "Keine früheren Fassungen.",
        "Write to ~/.ssh/config" => "In ~/.ssh/config schreiben",
        "Written to {files}; a copy of the old file is in ~/.ssh/.sshcraft-backup." => {
            "Geschrieben in {files}; eine Kopie der alten Datei liegt in ~/.ssh/.sshcraft-backup."
        }
        "Keep the hosts of ~/.ssh/config in step" => "Die Hosts aus ~/.ssh/config laufend übernehmen",
        "Its hosts and the files it includes appear in the group ~/.ssh/config and follow the file when it changes. A host that leaves the file stays here. Writing back happens only from a host's menu." => {
            "Ihre Hosts und die Dateien, die sie einbindet, erscheinen in der Gruppe ~/.ssh/config und folgen der Datei, wenn sie sich ändert. Ein Host, der aus der Datei verschwindet, bleibt hier. Zurückgeschrieben wird nur über das Menü eines Hosts."
        }
        "Host keys" => "Hostschlüssel",
        "Take new host keys a server announces" => "Neue Hostschlüssel übernehmen, die ein Server ankündigt",
        "After sign-in a server may name all its host keys. New ones are kept once the server proved it holds them, and keys it no longer has are dropped (UpdateHostKeys)." => {
            "Nach der Anmeldung kann ein Server alle seine Hostschlüssel nennen. Neue werden übernommen, sobald der Server bewiesen hat, dass er sie besitzt, und Schlüssel, die er nicht mehr hat, fallen weg (UpdateHostKeys)."
        }
        "Import known_hosts" => "known_hosts importieren",
        "Export known_hosts" => "known_hosts exportieren",
        "Servers whose host key SSHCraft trusts. Remove a key and the next connection asks again." => {
            "Server, deren Hostschlüssel SSHCraft vertraut. Wird ein Schlüssel entfernt, fragt die nächste Verbindung wieder nach."
        }
        "No known hosts yet." => "Noch keine bekannten Server.",
        "fingerprint only" => "nur Fingerabdruck",
        "The next connection asks again" => "Die nächste Verbindung fragt wieder nach",
        "{added} keys added, {same} already known, {conflicts} servers kept their known key." => {
            "{added} Schlüssel hinzugefügt, {same} schon bekannt, {conflicts} Server behalten ihren bekannten Schlüssel."
        }
        "Exported. {count} keys known only by their fingerprint were left out." => {
            "Exportiert. {count} Schlüssel, die nur mit Fingerabdruck bekannt sind, fehlen darin."
        }
        "Proxy command" => "Proxy-Befehl",
        "Runs without a shell. %h, %p, %r and %n stand for address, port, user and name." => {
            "Läuft ohne Shell. %h, %p, %r und %n stehen für Adresse, Port, Benutzer und Name."
        }
        "This needs a shell ({sign}); SSHCraft runs the command without one" => "Das braucht eine Shell ({sign}); SSHCraft startet den Befehl ohne",
        "the proxy command is too long" => "Der Proxy-Befehl ist zu lang",
        "a quote or a backslash is not closed" => "Ein Anführungszeichen oder Backslash ist nicht geschlossen",
        "Record sessions" => "Sitzungen aufzeichnen",
        "Remove" => "Entfernen",
        "Save" => "Speichern",
        "Search" => "Suchen",
        "Stored; type to replace" => "Gespeichert; zum Ersetzen tippen",
        "System" => "System",
        "Tags" => "Tags",
        "The port is a number from 1 to 65535." => "Der Port ist eine Zahl von 1 bis 65535.",
        "Top level" => "Oberste Ebene",
        "Unknown key" => "Unbekannter Schlüssel",
        "User" => "Benutzer",
        "Value" => "Wert",
        "{name} (copy)" => "{name} (Kopie)",
        "{value} (from {group})" => "{value} (von {group})",
        // Keys and identities
        "An identity needs a name." => "Eine Identität braucht einen Namen.",
        "Ask for PIN or fingerprint on each use" => "Bei jeder Nutzung nach PIN oder Fingerabdruck fragen",
        "Ask for the passphrase" => "Nach der Passphrase fragen",
        "Attach a certificate" => "Zertifikat anhängen",
        "Certificate" => "Zertifikat",
        "certificate" => "Zertifikat",
        "Copy public key" => "Öffentlichen Schlüssel kopieren",
        "Create" => "Anlegen",
        "Delete the key {name}? Hosts that use it can no longer sign in with it." => {
            "Den Schlüssel {name} löschen? Hosts, die ihn nutzen, können sich dann nicht mehr damit anmelden."
        }
        "Detach the certificate" => "Zertifikat entfernen",
        "Edit identity" => "Identität bearbeiten",
        "Generate" => "Erzeugen",
        "Generate key" => "Schlüssel erzeugen",
        "Identities" => "Identitäten",
        "Import" => "Importieren",
        "Import key" => "Schlüssel importieren",
        "Install on host" => "Auf Host eintragen",
        "New identity" => "Neue Identität",
        "No keys yet." => "Noch keine Schlüssel.",
        "Passphrase" => "Passphrase",
        "Private key" => "Privater Schlüssel",
        "Public key copied." => "Öffentlicher Schlüssel kopiert.",
        "Save the passphrase" => "Passphrase speichern",
        "Security key" => "Sicherheitsschlüssel",
        "Security key (FIDO2)" => "Sicherheitsschlüssel (FIDO2)",
        "Type" => "Typ",
        "Vault" => "Tresor",
        "Where" => "Wo",
        "Windows Hello" => "Windows Hello",
        "asks for passphrase" => "fragt nach Passphrase",
        "if the key has one" => "falls der Schlüssel eine hat",
        "optional" => "optional",
        // Import and export
        "~/.ssh/config" => "~/.ssh/config",
        "known_hosts file" => "known_hosts-Datei",
        "CSV file" => "CSV-Datei",
        "PuTTY (this computer)" => "PuTTY (dieser Computer)",
        "PuTTY registry export (.reg)" => "PuTTY-Registry-Export (.reg)",
        "MobaXterm (MobaXterm.ini)" => "MobaXterm (MobaXterm.ini)",
        "SecureCRT sessions folder" => "SecureCRT-Sitzungsordner",
        "{directive} is not followed" => "{directive} wird nicht befolgt",
        "{token} left as written" => "{token} bleibt, wie es steht",
        "the stored password is not taken over" => "das gespeicherte Passwort wird nicht übernommen",
        "the port was not valid and is left empty" => "der Port war ungültig und bleibt leer",
        "{what} is not taken over" => "{what} wird nicht übernommen",
        "Exported." => "Exportiert.",
        "{count} hosts created." => "{count} Hosts angelegt.",
        "Import from another program" => "Aus einem anderen Programm importieren",
        "Choose folder" => "Ordner wählen",
        "Choose file" => "Datei wählen",
        "Nothing chosen" => "Nichts gewählt",
        "Also take over the host keys (stored keys stay)" => "Auch die Host-Schlüssel übernehmen (gespeicherte bleiben)",
        "Nothing is created before Import hosts." => "Vor „Hosts importieren“ wird nichts angelegt.",
        "{count} lines skipped:" => "{count} Zeilen übersprungen:",
        "Preview" => "Vorschau",
        "Import hosts" => "Hosts importieren",
        "Export" => "Exportieren",
        "Export hosts" => "Hosts exportieren",
        "CSV (the values each host uses)" => "CSV (die Werte, die jeder Host nutzt)",
        "JSON (groups and inheritance)" => "JSON (Gruppen und Vererbung)",
        "Everything" => "Alles",
        "No password, passphrase or private key is exported." => "Kein Passwort, keine Passphrase und kein privater Schlüssel wird exportiert.",
        "Export to file" => "In Datei exportieren",
        // Session tabs and questions
        "Host" => "Host",
        "No local shell was found." => "Es wurde keine lokale Shell gefunden.",
        "No serial port was found." => "Es wurde kein serieller Port gefunden.",
        "Clipboard" => "Zwischenablage",
        "A program on {host} wants to put {count} characters in the clipboard. Allow it?" => {
            "Ein Programm auf {host} möchte {count} Zeichen in die Zwischenablage legen. Erlauben?"
        }
        "Once" => "Einmal",
        "Always for this host" => "Immer für diesen Host",
        "No" => "Nein",
        "Connecting…" => "Verbinde…",
        "Connected" => "Verbunden",
        "Connection lost. Reconnecting (attempt {attempt}) in {seconds} s." => "Verbindung verloren. Neuer Versuch ({attempt}) in {seconds} s.",
        "The session ended." => "Die Sitzung ist beendet.",
        "The connection was lost." => "Die Verbindung ist abgebrochen.",
        "The host key was not accepted." => "Der Host-Schlüssel wurde nicht angenommen.",
        "The host key of this server changed. Someone may be listening in, or the server was set up again. SSHCraft does not connect; if the change is expected, forget the old key in the host's settings." => {
            "Der Host-Schlüssel dieses Servers hat sich geändert. Vielleicht hört jemand mit, oder der Server wurde neu eingerichtet. SSHCraft verbindet nicht; ist die Änderung erwartet, vergiss den alten Schlüssel in den Einstellungen des Hosts."
        }
        "Sign-in failed." => "Die Anmeldung ist fehlgeschlagen.",
        "Sign-in was cancelled." => "Die Anmeldung wurde abgebrochen.",
        "The server could not be reached." => "Der Server war nicht erreichbar.",
        "The session could not start." => "Die Sitzung konnte nicht starten.",
        "Reconnect" => "Neu verbinden",
        "This session is gone." => "Diese Sitzung gibt es nicht mehr.",
        "Baud rate" => "Baudrate",
        "Unknown host key" => "Unbekannter Host-Schlüssel",
        "{host} ({address}) is not known yet. Compare the fingerprint with the one its administrator gave you before you accept it." => {
            "{host} ({address}) ist noch nicht bekannt. Vergleiche den Fingerabdruck mit dem, den dir die Verwaltung des Servers gegeben hat, bevor du ihn annimmst."
        }
        "Accept" => "Annehmen",
        "Sign in to {host}" => "Bei {host} anmelden",
        "Sign in" => "Anmelden",
        // Settings
        "General" => "Allgemein",
        "Appearance" => "Aussehen",
        "Shortcuts" => "Tastenkürzel",
        "SSHCraft dark" => "SSHCraft dunkel",
        "SSHCraft light" => "SSHCraft hell",
        "Language" => "Sprache",
        "Theme" => "Farbschema",
        "Dark" => "Dunkel",
        "Light" => "Hell",
        "Tab colour" => "Tab-Farbe",
        "Own colour" => "Eigene Farbe",
        "Badge" => "Abzeichen",
        "e.g. PROD" => "z. B. PROD",
        "Terminal theme" => "Terminal-Farbschema",
        "Terminal colours" => "Terminalfarben",
        "Terminal font" => "Terminalschrift",
        "Font size" => "Schriftgröße",
        "Hosts as" => "Hosts als",
        "Unknown host" => "Unbekannter Host",
        "Terminal type" => "Terminaltyp",
        "Local shell" => "Lokale Shell",
        "System default" => "Vorgabe des Systems",
        "History" => "Verlauf",
        "Keep the commands typed in terminals" => "In Terminals getippte Befehle behalten",
        "Suggestions" => "Vorschläge",
        "Suggest while typing" => "Beim Tippen vorschlagen",
        "Programs may write the clipboard (OSC 52), after asking" => "Programme dürfen nach Rückfrage in die Zwischenablage schreiben (OSC 52)",
        "Always allowed to write the clipboard" => "Dürfen immer in die Zwischenablage schreiben",
        "No host yet." => "Noch kein Host.",
        "{shortcut} is taken by {action}." => "{shortcut} gehört schon zu {action}.",
        "A shortcut needs Ctrl or Alt (⌘ on macOS), or is a function key." => {
            "Ein Tastenkürzel braucht Strg oder Alt (⌘ unter macOS), oder es ist eine Funktionstaste."
        }
        "Not a key combination: {why}" => "Keine Tastenkombination: {why}",
        "Inside a terminal only Ctrl+Shift and Ctrl+Tab are taken; every other key goes to the server." => {
            "Im Terminal gelten nur Strg+Umschalt und Strg+Tab; jede andere Taste geht an den Server."
        }
        "Press a key combination (Escape cancels)" => "Tastenkombination drücken (Escape bricht ab)",
        "{shortcut} (outside the terminal only)" => "{shortcut} (nur außerhalb des Terminals)",
        "Change" => "Ändern",
        "Default" => "Standard",
        // Keys from elsewhere
        "Import from Proton Pass" => "Aus Proton Pass importieren",
        "In the SSH agent" => "Im SSH-Agent",
        "Refresh" => "Aktualisieren",
        "The SSH agent cannot be reached. Is Proton Pass or another agent running?" => {
            "Der SSH-Agent ist nicht erreichbar. Läuft Proton Pass oder ein anderer Agent?"
        }
        "The SSH agent holds no key." => "Der SSH-Agent hat keinen Schlüssel.",
        "Agent key" => "Agent-Schlüssel",
        "pass-cli is not installed. Install Proton Pass's command line to import keys from it." => {
            "pass-cli ist nicht installiert. Installiere die Kommandozeile von Proton Pass, um Schlüssel daraus zu importieren."
        }
        "pass-cli is not signed in. Run pass-cli login, then try again." => {
            "pass-cli ist nicht angemeldet. Führe pass-cli login aus und versuche es dann erneut."
        }
        "pass-cli did not answer." => "pass-cli hat nicht geantwortet.",
        "Proton Pass holds no SSH key." => "Proton Pass enthält keinen SSH-Schlüssel.",
        "Key imported." => "Schlüssel importiert.",
        // R5
        "Asked by tab {number}: {title}" => "Frage von Tab {number}: {title}",
        "Show tab" => "Tab zeigen",
        "Ligatures" => "Ligaturen",
        "Join characters where the font has ligatures (->, !=)" => "Zeichen verbinden, wo die Schrift Ligaturen hat (->, !=)",
        "Scrollback" => "Zeilenspeicher",
        "attempts after a dropped connection, waiting up to a minute between them; 0 turns it off" => {
            "Versuche nach einem Verbindungsabbruch, mit bis zu einer Minute Pause dazwischen; 0 schaltet es aus"
        }
        "lines, for new tabs" => "Zeilen, für neue Tabs",
        "Snippets" => "Snippets",
        "Port forwarding" => "Portweiterleitung",
        "Logs" => "Protokolle",
        // Panes
        "New pane" => "Neuer Bereich",
        "Pane title" => "Bereichstitel",
        "Close pane" => "Bereich schließen",
        "Move to its own tab" => "In eigenen Tab verschieben",
        "Maximize" => "Groß zeigen",
        "Resize columns" => "Spaltenbreite ändern",
        "Resize rows" => "Zeilenhöhe ändern",
        "Tab" => "Tab",
        "A tab holds at most 16 panes." => "Ein Tab fasst höchstens 16 Bereiche.",
        "Split: add a pane" => "Teilen: Bereich hinzufügen",
        "Close the focused pane" => "Aktiven Bereich schließen",
        "Next pane" => "Nächster Bereich",
        "Previous pane" => "Vorheriger Bereich",
        "Label URLs, paths, addresses and hashes to copy them" => "URLs, Pfade, Adressen und Hashes beschriften, um sie zu kopieren",
        "Previous command" => "Vorheriger Befehl",
        "Next command" => "Nächster Befehl",
        "Copy the output of the last command" => "Ausgabe des letzten Befehls kopieren",
        "Running since {time}" => "Läuft seit {time}",
        "Exit code {code}, {took}, started {time}" => "Exit-Code {code}, {took}, gestartet {time}",
        "Finished, {took}, started {time}" => "Beendet, {took}, gestartet {time}",
        "Waiting for a command" => "Wartet auf einen Befehl",
        "Command finished on {host}" => "Befehl beendet auf {host}",
        "It succeeded." => "Er war erfolgreich.",
        "It ended with exit code {code}." => "Er endete mit Exit-Code {code}.",
        "It ended." => "Er ist beendet.",
        "Notifications" => "Benachrichtigungen",
        "Trigger" => "Trigger",
        "Highlights" => "Hervorhebungen",
        "Regular expressions coloured wherever they appear in a terminal, case-insensitive" => {
            "Reguläre Ausdrücke, die überall im Terminal eingefärbt werden, ohne Rücksicht auf Groß- und Kleinschreibung"
        }
        "Pattern" => "Muster",
        "Not a valid pattern" => "Kein gültiges Muster",
        "Add a highlight" => "Hervorhebung hinzufügen",
        "Triggers" => "Trigger",
        "Regular expressions checked once for each finished output line, case-insensitive" => {
            "Reguläre Ausdrücke, die einmal je fertiger Ausgabezeile geprüft werden, ohne Rücksicht auf Groß- und Kleinschreibung"
        }
        "Notify" => "Benachrichtigen",
        "Mark the tab" => "Tab markieren",
        "Add a trigger" => "Trigger hinzufügen",
        "seconds a command must run to notify when it ends in the background; 0 turns it off" => {
            "Sekunden, die ein Befehl laufen muss, damit sein Ende im Hintergrund gemeldet wird; 0 schaltet es ab"
        }
        "Servers may show desktop notifications, always with the host's name" => {
            "Server dürfen Desktop-Benachrichtigungen zeigen, immer mit dem Namen des Hosts"
        }
        "Switch between grid and one large pane" => "Zwischen Raster und einem großen Bereich wechseln",
        "Broadcast on" => "Rundsenden an",
        "Broadcast on or off" => "Rundsenden an oder aus",
        // Workspaces
        "Save as workspace" => "Als Workspace speichern",
        "Workspace saved." => "Workspace gespeichert.",
        "Title" => "Titel",
        "Directory" => "Verzeichnis",
        "Command" => "Befehl",
        "Over SSH the directory and the command are typed once after the first connect (a POSIX shell)." => {
            "Über SSH werden Verzeichnis und Befehl einmal nach dem ersten Verbinden getippt (eine POSIX-Shell)."
        }
        "{count} pane left out: its host was deleted." => "{count} Bereich weggelassen: sein Host wurde gelöscht.",
        "Workspaces" => "Workspaces",
        "Delete workspace" => "Workspace löschen",
        "Workspace: {name}" => "Workspace: {name}",
        // Port forwarding
        "Stopped" => "Gestoppt",
        "Starting…" => "Startet…",
        "Running on port {port}, {count} connections" => "Läuft auf Port {port}, {count} Verbindungen",
        ", {count} refused" => ", {count} abgelehnt",
        "Reconnecting (attempt {attempt}) in {seconds} s" => "Verbindet neu (Versuch {attempt}) in {seconds} s",
        "Failed: {detail}" => "Fehlgeschlagen: {detail}",
        "New rule" => "Neue Regel",
        "Edit rule" => "Regel bearbeiten",
        "No forwarding rules yet. A rule belongs to a host and starts here." => {
            "Noch keine Weiterleitungsregeln. Eine Regel gehört zu einem Host und startet hier."
        }
        "Reachable from other machines" => "Von anderen Rechnern erreichbar",
        "Start" => "Starten",
        "Stop" => "Stoppen",
        "Stop the rule to change it." => "Stoppe die Regel, um sie zu ändern.",
        "Without a rule" => "Ohne Regel",
        "Choose a host." => "Wähle einen Host.",
        "Choose a host" => "Host wählen",
        "A host has at most 50 rules." => "Ein Host hat höchstens 50 Regeln.",
        "The address has no spaces and at most 253 characters." => "Die Adresse hat keine Leerzeichen und höchstens 253 Zeichen.",
        "Ports are 1 to 65535, and the target needs an address." => "Ports sind 1 bis 65535, und das Ziel braucht eine Adresse.",
        "A name has at most 100 characters." => "Ein Name hat höchstens 100 Zeichen.",
        "Kind" => "Art",
        "Local (-L)" => "Lokal (-L)",
        "Remote (-R)" => "Entfernt (-R)",
        "Dynamic, SOCKS (-D)" => "Dynamisch, SOCKS (-D)",
        "Address on the server" => "Adresse auf dem Server",
        "Address here" => "Adresse hier",
        "Target here" => "Ziel hier",
        "Target from the server" => "Ziel vom Server aus",
        "Other machines can reach this port through {address}." => "Andere Rechner erreichen diesen Port über {address}.",
        "The SOCKS proxy asks no password: anyone who reaches the port can use it." => {
            "Der SOCKS-Proxy fragt kein Passwort: wer den Port erreicht, kann ihn nutzen."
        }
        // Snippets
        "A name is needed; name and package have at most 100 characters." => "Ein Name ist nötig; Name und Paket haben höchstens 100 Zeichen.",
        "The content has 1 to 10 000 characters." => "Der Inhalt hat 1 bis 10 000 Zeichen.",
        "The content has no control characters but line breaks and tabs." => "Der Inhalt hat keine Steuerzeichen außer Zeilenumbrüchen und Tabs.",
        "Snippet: {name}" => "Snippet: {name}",
        "New snippet" => "Neues Snippet",
        "Edit snippet" => "Snippet bearbeiten",
        "No snippets yet. They stay on this device and need the vault." => {
            "Noch keine Snippets. Sie bleiben auf diesem Gerät und brauchen den Tresor."
        }
        "Without package" => "Ohne Paket",
        "Run" => "Ausführen",
        "Package" => "Paket",
        "Content" => "Inhalt",
        "Default hosts" => "Standard-Hosts",
        "Run {name}" => "{name} ausführen",
        "{count} default host was deleted and is left out." => "{count} Standard-Host wurde gelöscht und fällt weg.",
        "Open panes" => "Offene Bereiche",
        "New connections" => "Neue Verbindungen",
        "Paste only" => "Nur einfügen",
        "Run on {count}" => "Auf {count} ausführen",
        "Run snippet" => "Snippet ausführen",
        "Clear" => "Leeren",
        "Snippets and history beside the terminal" => "Snippets und Verlauf neben dem Terminal",
        // Session logs
        "recording" => "nimmt auf",
        "Deleted host" => "Gelöschter Host",
        "A log still recording is not deleted." => "Ein Protokoll, das noch aufnimmt, wird nicht gelöscht.",
        "All logs" => "Alle Protokolle",
        "Local and serial" => "Lokal und seriell",
        "Search in all logs" => "In allen Protokollen suchen",
        "Delete selected" => "Auswahl löschen",
        "Delete all shown" => "Alle gezeigten löschen",
        "{count} hits" => "{count} Treffer",
        "No session logs. Recording is off unless a host or the settings turn it on." => {
            "Keine Sitzungsprotokolle. Aufgezeichnet wird nur, wenn ein Host oder die Einstellungen es einschalten."
        }
        "This device" => "Dieses Gerät",
        "Sync server" => "Sync-Server",
        "Export the log" => "Protokoll exportieren",
        "Text" => "Text",
        "Raw output" => "Rohe Ausgabe",
        "Session log" => "Sitzungsprotokoll",
        "Bookmark" => "Lesezeichen",
        "Comment" => "Kommentar",
        "This log is longer than the viewer keeps: its beginning is not shown, and marks are off." => {
            "Dieses Protokoll ist länger, als die Ansicht behält: sein Anfang wird nicht gezeigt, und Marken sind aus."
        }
        "Add comment" => "Kommentar hinzufügen",
        "Marks" => "Marken",
        "Remove mark" => "Marke entfernen",
        "Rec" => "Rec",
        "Bookmark added." => "Lesezeichen gesetzt.",
        "Bookmark in the session log" => "Lesezeichen im Sitzungsprotokoll",
        "Session logs" => "Sitzungsprotokolle",
        "Record sessions unless a host or group says otherwise" => "Sitzungen aufzeichnen, wenn Host oder Gruppe nichts anderes sagen",
        "Keep logs until deleted" => "Protokolle behalten, bis sie gelöscht werden",
        "Keep logs {days} days" => "Protokolle {days} Tage behalten",
        // Sync
        "Sync off" => "Sync aus",
        "Up to date" => "Aktuell",
        "Syncing…" => "Synchronisiert…",
        "Offline, retry in {seconds} s" => "Offline, neuer Versuch in {seconds} s",
        "This device was revoked" => "Dieses Gerät wurde widerrufen",
        "Blocked by the server's operator" => "Vom Betreiber des Servers gesperrt",
        "Error: {detail}" => "Fehler: {detail}",
        "No sync server yet. Everything stays on this device." => "Noch kein Sync-Server. Alles bleibt auf diesem Gerät.",
        "Sync" => "Sync",
        "Keys and passwords" => "Schlüssel und Passwörter",
        "Devices" => "Geräte",
        "Remove server" => "Server entfernen",
        "Create account" => "Konto anlegen",
        "Join an account" => "Einem Konto beitreten",
        "Connections SSHCraft makes by itself" => "Verbindungen, die SSHCraft von selbst aufbaut",
        "None. SSHCraft connects only where you tell it to." => "Keine. SSHCraft verbindet sich nur dorthin, wohin du es sagst.",
        "Sync server {origin}" => "Sync-Server {origin}",
        "Server name" => "Servername",
        "Server address" => "Serveradresse",
        "API key" => "API-Schlüssel",
        "Your name" => "Dein Name",
        "Email" => "E-Mail",
        "This device's name" => "Name dieses Geräts",
        "Join" => "Beitreten",
        "Account key" => "Kontoschlüssel",
        "Your account key" => "Dein Kontoschlüssel",
        "Keep it in your password manager now. SSHCraft does not store it and shows it only this once. Every new device needs it." => {
            "Lege ihn jetzt in deinem Passwortmanager ab. SSHCraft speichert ihn nicht und zeigt ihn nur dieses eine Mal. Jedes neue Gerät braucht ihn."
        }
        "Account key as QR code" => "Kontoschlüssel als QR-Code",
        "I have kept the key" => "Ich habe den Schlüssel abgelegt",
        "Account key (recovery)" => "Kontoschlüssel (Wiederherstellung)",
        "{name} (this device)" => "{name} (dieses Gerät)",
        "Revoked" => "Widerrufen",
        "Active" => "Aktiv",
        "Revoke" => "Widerrufen",
        "Revoke {name}? The account's keys change; SSH keys and passwords that device synced may have been copied: change them on the servers." => {
            "{name} widerrufen? Die Schlüssel des Kontos ändern sich; SSH-Schlüssel und Passwörter, die das Gerät synchronisiert hat, können kopiert worden sein: ändere sie auf den Servern."
        }
        "Revoke the device" => "Gerät widerrufen",
        "Move {name}" => "{name} verschieben",
        "This device only" => "Nur dieses Gerät",
        "Move" => "Verschieben",
        "Move to…" => "Verschieben nach…",
        // Sharing
        "Move it to a sync server first: shares live there." => "Verschiebe es zuerst auf einen Sync-Server: dort liegen Freigaben.",
        "Shared." => "Geteilt.",
        "Share {name}" => "{name} teilen",
        "Only with accounts on the same server." => "Nur mit Konten auf demselben Server.",
        "Find" => "Suchen",
        "{name} ({email})" => "{name} ({email})",
        "The key of this account changed. Before: {old}. Now: {new}. Ask {name} whether they set up their account again." => {
            "Der Schlüssel dieses Kontos hat sich geändert. Vorher: {old}. Jetzt: {new}. Frag {name}, ob das Konto neu eingerichtet wurde."
        }
        "I compared the new safety number with {name}" => "Ich habe die neue Sicherheitsnummer mit {name} verglichen",
        "Their safety number: {number}" => "Ihre Sicherheitsnummer: {number}",
        "Compare it with them by phone or in person; it must be the same on their device." => {
            "Vergleiche sie telefonisch oder persönlich; auf dem anderen Gerät muss sie gleich sein."
        }
        "I compared the safety number with {name}" => "Ich habe die Sicherheitsnummer mit {name} verglichen",
        "Read" => "Lesen",
        "Read and change" => "Lesen und ändern",
        "Share the keys too (never keys in the SSH agent)" => "Auch die Schlüssel teilen (nie Schlüssel im SSH-Agent)",
        "Share" => "Teilen",
        "Shares" => "Freigaben",
        "Nothing shared." => "Nichts geteilt.",
        "shared by you" => "von dir geteilt",
        "shared with you" => "mit dir geteilt",
        "End share" => "Freigabe beenden",
        "Unknown account" => "Unbekanntes Konto",
        "read and change" => "lesen und ändern",
        "read" => "lesen",
        "Remove recipient" => "Empfänger entfernen",
        "They lose access, and the share gets a new key." => "Der Zugriff endet, und die Freigabe bekommt einen neuen Schlüssel.",
        "Keys went along with this share: they may have been copied. Change them on the servers." => {
            "Mit dieser Freigabe gingen Schlüssel mit: sie können kopiert worden sein. Ändere sie auf den Servern."
        }
        "Share…" => "Teilen…",
        // AI agents
        "View hosts, groups and key names" => "Hosts, Gruppen und Schlüsselnamen ansehen",
        "Connect and run commands" => "Verbinden und Befehle ausführen",
        "Create and change hosts and groups" => "Hosts und Gruppen anlegen und ändern",
        "Import and generate keys" => "Schlüssel importieren und erzeugen",
        "Sync servers and devices" => "Sync-Server und Geräte",
        "allowed" => "erlaubt",
        "asked and allowed" => "gefragt und erlaubt",
        "asked and denied" => "gefragt und abgelehnt",
        "blocked" => "gesperrt",
        "hidden" => "verborgen",
        "cancelled" => "abgebrochen",
        "failed" => "fehlgeschlagen",
        "What AI agents may do through MCP or the command line. \"Ask\" asks in this window; with no window, the answer is no." => {
            "Was KI-Agenten über MCP oder die Kommandozeile dürfen. \"Fragen\" fragt in diesem Fenster; ohne Fenster ist die Antwort nein."
        }
        "Allow" => "Erlauben",
        "Ask" => "Fragen",
        "Block" => "Sperren",
        "Hidden from agents" => "Vor Agenten verborgen",
        "Nothing. Hide a host or group from its context menu." => "Nichts. Verbirg einen Host oder eine Gruppe über sein Kontextmenü.",
        "Show again" => "Wieder zeigen",
        "Log" => "Protokoll",
        "Clear the log" => "Protokoll leeren",
        "AI agents" => "KI-Agenten",
        "This command has more than one line: read every line." => "Dieser Befehl hat mehr als eine Zeile: lies jede Zeile.",
        "This command is long: read all of it." => "Dieser Befehl ist lang: lies ihn ganz.",
        "This command has hidden or reordering characters, shown as ⟨U+…⟩." => {
            "Dieser Befehl hat verborgene oder umordnende Zeichen, gezeigt als ⟨U+…⟩."
        }
        "An AI agent asks" => "Ein KI-Agent fragt",
        "Agent" => "Agent",
        "Action" => "Aktion",
        "Target" => "Ziel",
        "Deny" => "Ablehnen",
        "Hide from agents" => "Vor Agenten verbergen",
        "Delete {count} session logs?" => "{count} Sitzungsprotokolle löschen?",
        "They are deleted for good, on the sync server and your other devices too." => {
            "Sie werden endgültig gelöscht, auch auf dem Sync-Server und deinen anderen Geräten."
        }
        "Try again" => "Noch einmal",
        "This device leaves the sync server and forgets its sign-in. Joining again needs the account key, which SSHCraft never keeps." => {
            "Dieses Gerät verlässt den Sync-Server und vergisst seine Anmeldung. Für einen neuen Beitritt braucht es den Kontoschlüssel, den SSHCraft nie aufbewahrt."
        }
        "Remove and sign out" => "Entfernen und abmelden",
        "Every recipient loses access to these hosts at once." => "Alle Empfänger verlieren sofort den Zugriff auf diese Hosts.",
        "End sharing" => "Teilen beenden",
        "Delete {name}?" => "{name} löschen?",
        "The snippet is deleted for good, on your other devices too." => "Das Snippet wird endgültig gelöscht, auch auf deinen anderen Geräten.",
        "Delete snippet" => "Snippet löschen",
        "Delete the rule {name}?" => "Die Regel {name} löschen?",
        "A running rule stops; the rule is gone from its host." => "Eine laufende Regel stoppt; die Regel verschwindet von ihrem Host.",
        "Delete rule" => "Regel löschen",
        "The last change is still being saved: try again in a moment." => "Die letzte Änderung wird noch gespeichert: versuch es gleich noch einmal.",
        "Search in this log" => "In diesem Protokoll suchen",
        "Previous" => "Vorheriger",
        "Next" => "Nächster",
        "Not found in this log." => "In diesem Protokoll nicht gefunden.",
        "Connection" => "Verbindung",
        "The port is a number from 0 to 65535." => "Der Port ist eine Zahl von 0 bis 65535.",
        "Copy" => "Kopieren",
        "Allow agent control" => "Steuerung durch Agenten erlauben",
        "Port (0: any free port)" => "Port (0: ein freier Port)",
        "Agents can connect on 127.0.0.1:{port}." => "Agenten können sich über 127.0.0.1:{port} verbinden.",
        "Agents cannot connect: {error}" => "Agenten können sich nicht verbinden: {error}",
        "Turned off in the ArtCraft Launcher (Settings › AI agents)." => "Im ArtCraft Launcher ausgeschaltet (Einstellungen › KI-Agenten).",
        "Agents cannot connect." => "Agenten können sich nicht verbinden.",
        "SSHCraft is already open. Switch to its window." => "SSHCraft ist schon geöffnet. Wechsle in sein Fenster.",
        "The previous version of SSHCraft is still running. Quit it (also in the notification area), then start SSHCraft again." => {
            "Die vorige Fassung von SSHCraft läuft noch. Beende sie (auch im Infobereich der Taskleiste) und starte SSHCraft dann erneut."
        }
        "SSHCraft could not open its data." => "SSHCraft konnte seine Daten nicht öffnen.",
        "SSHCraft could not start." => "SSHCraft konnte nicht starten.",
        // The menus, the title bar and quitting (R7).
        "Quit SSHCraft?" => "SSHCraft beenden?",
        "End sessions and quit" => "Sitzungen beenden und schließen",
        "Quit" => "Beenden",
        "Broadcast" => "Übertragen an alle",
        "About SSHCraft" => "Über SSHCraft",
        "File" => "Datei",
        "View" => "Ansicht",
        "Session" => "Sitzung",
        "Window" => "Fenster",
        "Help" => "Hilfe",
        "Quit SSHCraft" => "SSHCraft beenden",
        "Exit" => "Beenden",
        "Preferences…" => "Einstellungen …",
        "Preferences" => "Einstellungen",
        "About {app}" => "Über {app}",
        "Settings…" => "Einstellungen …",
        "Hide {app}" => "{app} ausblenden",
        "Hide Others" => "Andere ausblenden",
        "Quit {app}" => "{app} beenden",
        "Minimize" => "Minimieren",
        "Restore" => "Wiederherstellen",
        "Close window" => "Fenster schließen",
        "Use system title bar" => "Titelleiste des Systems verwenden",
        "The desktop draws the title bar. Takes effect after a restart." => "Der Desktop zeichnet die Titelleiste. Wirkt nach einem Neustart.",
        "Version {version}" => "Version {version}",
        "An SSH client with end-to-end encrypted, self-hosted sync." => "Ein SSH-Client mit Ende-zu-Ende-verschlüsseltem, selbst gehostetem Sync.",
        "Licence: {licence}" => "Lizenz: {licence}",
        "Menu" => "Menü",
        "Scroll menu up" => "Menü nach oben scrollen",
        "Scroll menu down" => "Menü nach unten scrollen",
        "Show All" => "Alle einblenden",
        "Services" => "Dienste",
        "Zoom" => "Zoomen",
        "Bring All to Front" => "Alle nach vorne bringen",
        "SSHCraft's entry in the system keyring is missing, so stored keys, passwords and logs cannot be read. Nothing was replaced: restore the keyring entry or the data folder from a backup." => {
            "Der Eintrag von SSHCraft im Schlüsselbund des Systems fehlt, deshalb lassen sich gespeicherte Schlüssel, Passwörter und Protokolle nicht lesen. Ersetzt wurde nichts: Stelle den Eintrag oder den Datenordner aus einer Sicherung wieder her."
        }
        "The system keyring could not be opened, so keys and passwords are not available. On Linux a Secret Service (GNOME Keyring or KWallet) must run." => {
            "Der Schlüsselbund des Systems ließ sich nicht öffnen, deshalb sind Schlüssel und Passwörter nicht verfügbar. Unter Linux muss ein Secret Service laufen (GNOME Keyring oder KWallet)."
        }
        "Still running: {what}. Quitting ends it." => "Es läuft noch: {what}. Mit dem Beenden endet das.",
        "1 session" => "1 Sitzung",
        "{n} sessions" => "{n} Sitzungen",
        "1 port forward" => "1 Portweiterleitung",
        "{n} port forwards" => "{n} Portweiterleitungen",
        " and " => " und ",
        _ => return None,
    })
}
