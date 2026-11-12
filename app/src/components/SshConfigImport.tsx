import { useEffect, useState } from "react";
import { sshConfigImport, sshConfigPreview, type ImportCandidate, type SshConfigPreview } from "../api";
import { errorText } from "../i18n/errors";
import { useT, type Translate } from "../i18n/i18n";

type Props = { onDone: () => void; onCancel: () => void };

function summary(t: Translate, c: ImportCandidate): string {
  const parts = [c.group ? t("sshconfig.group", { name: c.group }) : (c.settings.address ?? "")];
  if (c.settings.username) parts.push(t("sshconfig.user", { name: c.settings.username }));
  if (c.settings.port) parts.push(t("sshconfig.port", { port: c.settings.port }));
  return parts.join(", ");
}

/** Shows the hosts found in ~/.ssh/config and imports the selected ones. */
export function SshConfigImport({ onDone, onCancel }: Props) {
  const t = useT();
  const [preview, setPreview] = useState<SshConfigPreview | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<number | null>(null);

  useEffect(() => {
    sshConfigPreview().then(
      (p) => {
        setPreview(p);
        // Candidates with warnings would behave differently than in OpenSSH: opt in only.
        setSelected(new Set(p.candidates.filter((c) => c.warnings.length === 0).map((c) => c.alias)));
      },
      (e) => setError(errorText(t, e)),
    );
  }, [t]);

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
      setCreated(result.created);
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  if (created !== null) {
    return (
      <div className="dialog">
        <h2>{t("sshconfig.title")}</h2>
        <div>{t("sshconfig.created", { count: created })}</div>
        <div className="buttons">
          <button className="primary" onClick={onDone}>
            {t("common.close")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="dialog">
      <h2>{t("sshconfig.title")}</h2>
      {!preview && !error && <div>{t("sshconfig.reading")}</div>}
      {preview && preview.candidates.length === 0 && <div className="empty">{t("sshconfig.none")}</div>}
      {preview?.candidates.map((c) => (
        <label key={c.alias} className="key-row candidate-row">
          <input type="checkbox" checked={selected.has(c.alias)} onChange={() => toggle(c.alias)} />
          <span className="candidate-text">
            <strong>{c.alias}</strong>
            <span>{summary(t, c)}</span>
            <span>
              {c.settings.key?.kind === "agent"
                ? t("sshconfig.keyFromAgent")
                : c.key_note && t(`sshconfig.keyNote.${c.key_note}`)}
            </span>
            {c.warnings.map((w, index) => (
              <span key={index} className="warning-text">
                {w.kind === "proxy_ignored"
                  ? t("sshconfig.warning.proxy_ignored", { directive: w.directive })
                  : t("sshconfig.warning.unsupported_token", { token: w.token })}
              </span>
            ))}
          </span>
        </label>
      ))}
      {preview && preview.skipped.length > 0 && (
        <div>
          <strong>{t("sshconfig.skipped.title")}</strong> {t("sshconfig.skipped.hint")}
          {/* The same Include line may appear more than once, so the index is the key. */}
          {preview.skipped.map((line, index) => (
            <div key={index} className="mono">
              {line}
            </div>
          ))}
        </div>
      )}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={busy || selected.size === 0} onClick={() => void submit()}>
          {t("sshconfig.import")}
        </button>
      </div>
    </div>
  );
}
