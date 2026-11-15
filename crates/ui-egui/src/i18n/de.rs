//! German. One arm per English text; `tests/i18n.rs` fails when a text of the interface has none.

pub fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Close" => "Schließen",
        "Hosts" => "Hosts",
        "Keys" => "Schlüssel",
        "Settings" => "Einstellungen",
        "Terminal" => "Terminal",
        "{count} hosts" => "{count} Hosts",
        "SSHCraft is already open. Switch to its window." => "SSHCraft ist schon geöffnet. Wechsle in sein Fenster.",
        "The previous version of SSHCraft is still running. Quit it (also in the notification area), then start SSHCraft again." => {
            "Die vorige Fassung von SSHCraft läuft noch. Beende sie (auch im Infobereich der Taskleiste) und starte SSHCraft dann erneut."
        }
        "SSHCraft could not open its data." => "SSHCraft konnte seine Daten nicht öffnen.",
        "SSHCraft could not start." => "SSHCraft konnte nicht starten.",
        _ => return None,
    })
}
