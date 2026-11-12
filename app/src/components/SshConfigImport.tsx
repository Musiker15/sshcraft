import { useEffect, useState } from "react";
import { sshConfigImport, sshConfigPreview, type SshConfigPreview } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

type Props = { onDone: () => void; onCancel: () => void };

/** Shows the hosts found in ~/.ssh/config and imports the selected ones. */
export function SshConfigImport({ onDone, onCancel }: Props) {
  const t = useT();
  const [preview, setPreview] = useState<SshConfigPreview | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    sshConfigPreview().then(
      (p) => {
        setPreview(p);
        setSelected(new Set(p.candidates.map((c) => c.alias)));
      },
      (e) => setError(errorText(t, e)),
    );
  }, []);

  function toggle(alias: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(alias)) next.delete(alias);
      else next.add(alias);
      return next;
    });
  }

  async function submit() {
    setBusy(true);
    try {
      if (!preview) return;
      // Send back exactly what was shown, so nothing changes between preview and import.
      const result = await sshConfigImport(preview.candidates.filter((c) => selected.has(c.alias)));
      alert(`${result.created} Host(s) angelegt.`);
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog">
      <h2>Aus ~/.ssh/config importieren</h2>
      {!preview && !error && <div>Lese ssh-config …</div>}
      {preview && preview.candidates.length === 0 && <div className="empty">Keine Hosts gefunden.</div>}
      {preview?.candidates.map((c) => (
        <label key={c.alias} className="key-row" style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
          <input
            type="checkbox"
            style={{ width: "auto" }}
            checked={selected.has(c.alias)}
            onChange={() => toggle(c.alias)}
          />
          <span style={{ display: "grid", gap: 2 }}>
            <strong style={{ color: "var(--text)" }}>{c.alias}</strong>
            <span>
              {c.group ? `Gruppe ${c.group}` : c.settings.address}
              {c.settings.username ? `, Benutzer ${c.settings.username}` : ""}
              {c.settings.port ? `, Port ${c.settings.port}` : ""}
            </span>
            <span>{c.settings.key?.kind === "agent" ? "Schlüssel aus Proton Pass" : c.key_note}</span>
          </span>
        </label>
      ))}
      {preview && preview.skipped.length > 0 && (
        <div>
          <strong>Nicht übernommen</strong> (Include und Match werden nicht ausgewertet):
          {preview.skipped.map((line) => (
            <div key={line} className="mono">
              {line}
            </div>
          ))}
        </div>
      )}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>Abbrechen</button>
        <button className="primary" disabled={busy || selected.size === 0} onClick={() => void submit()}>
          Importieren
        </button>
      </div>
    </div>
  );
}
