import { useEffect, useState } from "react";
import { createSecurityKey, generateKey, importKey, securityKeySupport, type EcdsaCurve, type SkAlgorithm } from "../api";
import { generateRequest, type KeyType } from "../forms/keyForm";
import { errorText, FormError } from "../i18n/errors";
import { useT } from "../i18n/i18n";

type Props = { onDone: () => void; onCancel: () => void };

/** Key parameters, written the same in every language. */
const CURVES: EcdsaCurve[] = ["nistp256", "nistp384", "nistp521"];
const RSA_BITS = [3072, 4096] as const;

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
  const [save, setSave] = useState(true);
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
      {passphrase && (
        <label className="check">
          <input type="checkbox" checked={save} onChange={(e) => setSave(e.target.checked)} />
          {t("keys.savePassphrase")}
        </label>
      )}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          disabled={busy || !text.trim()}
          onClick={() => void submit(() => importKey(required(name), text, passphrase || null, save))}
        >
          {t("keys.import")}
        </button>
      </div>
    </div>
  );
}

/** Create a new Ed25519, ECDSA or RSA key inside the vault, with or without a passphrase. */
export function KeyGenerateForm({ onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState("");
  const [type, setType] = useState<KeyType>("ed25519");
  const [curve, setCurve] = useState<EcdsaCurve>("nistp256");
  const [bits, setBits] = useState<3072 | 4096>(3072);
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [save, setSave] = useState(true);
  const { error, busy, submit } = useSubmit(onDone);
  return (
    <div className="dialog">
      <h2>{t("keys.generateTitle")}</h2>
      <p className="hint">{t("keys.generateHint")}</p>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label>
        {t("keys.type")}
        <select value={type} onChange={(e) => setType(e.target.value as KeyType)}>
          <option value="ed25519">{t("keys.typeEd25519")}</option>
          <option value="ecdsa">{t("keys.typeEcdsa")}</option>
          <option value="rsa">{t("keys.typeRsa")}</option>
        </select>
      </label>
      {type === "ecdsa" && (
        <label>
          {t("keys.curve")}
          <select value={curve} onChange={(e) => setCurve(e.target.value as EcdsaCurve)}>
            {CURVES.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
      )}
      {type === "rsa" && (
        <label>
          {t("keys.bits")}
          <select value={bits} onChange={(e) => setBits(Number(e.target.value) as 3072 | 4096)}>
            {RSA_BITS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>
      )}
      <label>
        {t("keys.newPassphrase")}
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
      </label>
      {passphrase && (
        <>
          <label>
            {t("keys.repeatPassphrase")}
            <input type="password" value={repeat} onChange={(e) => setRepeat(e.target.value)} />
          </label>
          <label className="check">
            <input type="checkbox" checked={save} onChange={(e) => setSave(e.target.checked)} />
            {t("keys.savePassphrase")}
          </label>
        </>
      )}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          disabled={busy}
          onClick={() =>
            void submit(() => {
              const request = generateRequest({ name, type, curve, bits, passphrase, repeat, save });
              return generateKey(request.name, request.spec, request.passphrase);
            })
          }
        >
          {t("keys.generate")}
        </button>
      </div>
    </div>
  );
}

/** A key on a FIDO2 token or in Windows Hello. Windows shows its own dialog meanwhile. */
export function SecurityKeyForm({ onDone, onCancel }: Props) {
  const t = useT();
  const [support, setSupport] = useState<{ fido2: boolean; hello: boolean } | null>(null);
  const [name, setName] = useState("");
  const [kind, setKind] = useState<"fido2" | "hello">("fido2");
  const [algorithm, setAlgorithm] = useState<SkAlgorithm>("ecdsa_p256");
  const [verify, setVerify] = useState(true);
  const { error, busy, submit } = useSubmit(onDone);
  useEffect(() => {
    securityKeySupport().then(setSupport, () => setSupport({ fido2: false, hello: false }));
  }, []);
  const unsupported = support !== null && !support.fido2;
  return (
    <div className="dialog">
      <h2>{t("keys.securityTitle")}</h2>
      {unsupported && <div className="error">{t("errors.not_supported")}</div>}
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label className="check">
        <input type="radio" name="kind" checked={kind === "fido2"} onChange={() => setKind("fido2")} />
        {t("keys.kindFido2")}
      </label>
      <label className="check">
        <input
          type="radio"
          name="kind"
          checked={kind === "hello"}
          disabled={!support?.hello}
          onChange={() => setKind("hello")}
        />
        {t("keys.kindHello")}
      </label>
      {kind === "fido2" ? (
        <>
          <label>
            {t("keys.algorithm")}
            <select value={algorithm} onChange={(e) => setAlgorithm(e.target.value as SkAlgorithm)}>
              <option value="ecdsa_p256">{t("keys.algEcdsa")}</option>
              <option value="ed25519">{t("keys.algEd25519")}</option>
            </select>
          </label>
          <label className="check">
            <input type="checkbox" checked={verify} onChange={(e) => setVerify(e.target.checked)} />
            {t("keys.verifyUser")}
          </label>
        </>
      ) : (
        <p className="hint">{t("keys.helloHint")}</p>
      )}
      <p className="hint">{t("keys.serverNeeds")}</p>
      {busy && <p className="hint">{t("keys.touchHint")}</p>}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          disabled={busy || unsupported || support === null}
          onClick={() => void submit(() => createSecurityKey(required(name), kind, algorithm, verify))}
        >
          {t("keys.generate")}
        </button>
      </div>
    </div>
  );
}
