import { useState } from "react";
import { generateKey, importKey } from "../api";
import { errorText, FormError } from "../i18n/errors";
import { useT } from "../i18n/i18n";

type Props = { onDone: () => void; onCancel: () => void };

/** The first line of an OpenSSH private key, the same in every language. */
const KEY_HEADER = "-----BEGIN OPENSSH PRIVATE KEY-----";

function useSubmit(onDone: () => void) {
  const t = useT();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  async function submit(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }
  return { error, busy, submit };
}

function required(name: string): string {
  if (!name.trim()) throw new FormError("name_required");
  return name.trim();
}

/** Paste or pick an OpenSSH private key and seal it into the vault. */
export function KeyImportForm({ onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const { error, busy, submit } = useSubmit(onDone);

  async function readFile(file: File | undefined) {
    if (!file) return;
    setText(await file.text());
    if (!name.trim()) setName(file.name);
  }

  return (
    <div className="dialog">
      <h2>{t("keys.importTitle")}</h2>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label>
        {t("keys.privateKey")}
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={KEY_HEADER}
          spellCheck={false}
        />
      </label>
      <input type="file" aria-label={t("keys.chooseFile")} onChange={(e) => void readFile(e.target.files?.[0])} />
      <label>
        {t("keys.passphrase")}
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
      </label>
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          disabled={busy || !text.trim()}
          onClick={() => void submit(() => importKey(required(name), text, passphrase || null))}
        >
          {t("keys.import")}
        </button>
      </div>
    </div>
  );
}

/** Create a new Ed25519 key inside the vault. */
export function KeyGenerateForm({ onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState("");
  const { error, busy, submit } = useSubmit(onDone);
  return (
    <div className="dialog">
      <h2>{t("keys.generateTitle")}</h2>
      <p className="hint">{t("keys.generateHint")}</p>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={busy} onClick={() => void submit(() => generateKey(required(name)))}>
          {t("keys.generate")}
        </button>
      </div>
    </div>
  );
}
