import { useState } from "react";
import { deleteKey, errorMessage, generateKey, importKey, type KeyInfo } from "../api";

type Props = { keys: KeyInfo[]; onChanged: () => void; onClose: () => void };

export function KeyManager({ keys, onChanged, onClose }: Props) {
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setText("");
      setPassphrase("");
      setName("");
      onChanged();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function requireName(): string {
    if (!name.trim()) throw new Error("Bitte einen Namen für den Schlüssel eingeben.");
    return name.trim();
  }

  async function readFile(file: File | undefined) {
    if (!file) return;
    setText(await file.text());
    if (!name.trim()) setName(file.name);
  }

  return (
    <div className="dialog">
      <h2>Schlüssel</h2>
      {keys.length === 0 && <p className="empty">Noch keine Schlüssel.</p>}
      {keys.map((k) => (
        <div key={k.id} className="key-row">
          <div className="head">
            <strong>{k.name}</strong>
            <span>
              <button className="link" onClick={() => navigator.clipboard.writeText(k.public_key)}>
                Öffentlichen Schlüssel kopieren
              </button>
              <button
                className="link danger"
                onClick={() => {
                  if (confirm(`Schlüssel "${k.name}" löschen?`)) void run(() => deleteKey(k.id));
                }}
              >
                Löschen
              </button>
            </span>
          </div>
          <span className="mono">{k.algorithm}</span>
          <span className="mono">{k.fingerprint}</span>
        </div>
      ))}

      <h2>Schlüssel hinzufügen</h2>
      <label>
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        Privater Schlüssel (OpenSSH), einfügen oder Datei wählen
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
          spellCheck={false}
        />
      </label>
      <input type="file" onChange={(e) => void readFile(e.target.files?.[0])} />
      <label>
        Passphrase (falls der Schlüssel verschlüsselt ist)
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
      </label>
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button disabled={busy} onClick={() => void run(() => generateKey(requireName()))}>
          Neuen Ed25519-Schlüssel erzeugen
        </button>
        <button
          className="primary"
          disabled={busy || !text.trim()}
          onClick={() => void run(() => importKey(requireName(), text, passphrase || null))}
        >
          Importieren
        </button>
      </div>
      <div className="buttons">
        <button onClick={onClose}>Schließen</button>
      </div>
    </div>
  );
}
