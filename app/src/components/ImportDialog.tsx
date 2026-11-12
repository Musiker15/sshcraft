import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import {
  importApply,
  importDefaults,
  importKeyFile,
  importKnownHosts,
  importPreview,
  scanKeyFiles,
  type ImportCandidate,
  type ImportDefaults,
  type ImportPreview,
  type ImportSource,
  type ImportWarning,
  type KeyFile,
  type KnownOutcome,
  type ServerView,
} from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

export type SourceKind =
  | "ssh_config"
  | "known_hosts"
  | "key_files"
  | "csv"
  | "putty_registry"
  | "putty_reg"
  | "moba_xterm"
  | "secure_crt";

const NEEDS_FOLDER: SourceKind[] = ["key_files", "secure_crt"];

type Props = {
  servers: ServerView[];
  initial?: SourceKind;
  onDone: () => void;
  onCancel: () => void;
};

type Result = { created: number; duplicates: number; known?: KnownOutcome; keys?: number; failedKeys?: number };

function defaultPath(kind: SourceKind, defaults: ImportDefaults | null): string {
  if (!defaults) return "";
  switch (kind) {
    case "known_hosts":
      return defaults.known_hosts ?? "";
    case "key_files":
      return defaults.ssh_dir ?? "";
    case "moba_xterm":
      return defaults.mobaxterm ?? "";
    case "secure_crt":
      return defaults.securecrt ?? "";
    default:
      return "";
  }
}

function source(kind: SourceKind, path: string): ImportSource | null {
  switch (kind) {
    case "ssh_config":
      return { kind: "ssh_config" };
    case "putty_registry":
      return { kind: "putty_registry" };
    case "known_hosts":
    case "csv":
    case "putty_reg":
    case "moba_xterm":
      return { kind, path };
    case "secure_crt":
      return { kind: "secure_crt", dir: path };
    case "key_files":
      return null;
  }
}

/** Hosts, host keys or key files from other programs: pick a source, check the preview, import. */
export function ImportDialog({ servers, initial = "ssh_config", onDone, onCancel }: Props) {
  const t = useT();
  const [defaults, setDefaults] = useState<ImportDefaults | null>(null);
  const [kind, setKind] = useState<SourceKind>(initial);
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [keyFiles, setKeyFiles] = useState<KeyFile[] | null>(null);
  const [passphrases, setPassphrases] = useState<Record<string, string>>({});
  const [location, setLocation] = useState("");
  const [takeHostKeys, setTakeHostKeys] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<Result | null>(null);

  useEffect(() => {
    importDefaults().then((d) => {
      setDefaults(d);
      setPath((current) => current || defaultPath(initial, d));
    }, () => {});
  }, [initial]);

  const needsPath = kind !== "ssh_config" && kind !== "putty_registry";

  function choose(next: SourceKind) {
    setKind(next);
    setPath(defaultPath(next, defaults));
    setPreview(null);
    setKeyFiles(null);
    setError(null);
  }

  async function browse() {
    const picked = await open({ directory: NEEDS_FOLDER.includes(kind), multiple: false });
    if (typeof picked === "string") setPath(picked);
  }

  async function load() {
    setError(null);
    setBusy(true);
    try {
      if (kind === "key_files") {
        setKeyFiles(await scanKeyFiles(path));
        return;
      }
      const from = source(kind, path);
      if (!from) return;
      const p = await importPreview(from);
      setPreview(p);
      // Candidates with warnings would behave differently than in their source: opt in only.
      setSelected(new Set(p.plan.candidates.flatMap((c, i) => (c.warnings.length === 0 ? [i] : []))));
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  async function submit() {
    setError(null);
    setBusy(true);
    try {
      if (kind === "key_files" && keyFiles) {
        let keys = 0;
        let failedKeys = 0;
        for (const file of keyFiles) {
          if (!selected.has(keyFiles.indexOf(file))) continue;
          try {
            await importKeyFile(file.path, file.encrypted ? (passphrases[file.path] ?? "") : null);
            keys++;
          } catch {
            failedKeys++;
          }
        }
        setResult({ created: 0, duplicates: 0, keys, failedKeys });
        return;
      }
      if (!preview) return;
      // Send back exactly what was shown, so nothing changes between preview and import.
      const chosen = preview.plan.candidates.filter((_, i) => selected.has(i));
      const outcome = await importApply(chosen, location || null);
      const known = kind === "known_hosts" && takeHostKeys ? await importKnownHosts(path) : undefined;
      setResult({ created: outcome.created, duplicates: outcome.duplicates, known });
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  function toggle(index: number) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  }

  const warning = (w: ImportWarning) => {
    switch (w.kind) {
      case "proxy_ignored":
        return t("sshconfig.warning.proxy_ignored", { directive: w.directive });
      case "unsupported_token":
        return t("sshconfig.warning.unsupported_token", { token: w.token });
      case "password_not_imported":
        return t("import.warning.password_not_imported");
      case "bad_port":
        return t("import.warning.bad_port");
      case "unsupported":
        return t("import.warning.unsupported", { what: w.what });
    }
  };

  const describe = (c: ImportCandidate) => {
    const parts = [c.group_path.length ? c.group_path.join(" / ") : null, c.settings.address];
    if (c.settings.username) parts.push(t("sshconfig.user", { name: c.settings.username }));
    if (c.settings.port) parts.push(t("sshconfig.port", { port: c.settings.port }));
    return parts.filter(Boolean).join(" · ");
  };

  if (result) {
    return (
      <div className="dialog">
        <h2>{t("import.title")}</h2>
        {result.keys !== undefined ? (
          <p>{t("import.keys", { count: result.keys })}</p>
        ) : (
          <p>{t("import.created", { count: result.created })}</p>
        )}
        {result.duplicates > 0 && <p>{t("import.duplicates", { count: result.duplicates })}</p>}
        {(result.failedKeys ?? 0) > 0 && <p className="warning-text">{t("import.keysFailed", { count: result.failedKeys ?? 0 })}</p>}
        {result.known && <p>{t("import.hostKeys", { count: result.known.added })}</p>}
        {result.known && result.known.conflicts.length > 0 && (
          <p className="warning-text">
            {t("import.hostKeyConflicts", {
              hosts: result.known.conflicts.map(([h, p]) => (p === 22 ? h : `${h}:${p}`)).join(", "),
            })}
          </p>
        )}
        <div className="buttons">
          <button className="primary" onClick={onDone}>
            {t("common.close")}
          </button>
        </div>
      </div>
    );
  }

  const kinds: SourceKind[] = [
    "ssh_config",
    "known_hosts",
    "key_files",
    "csv",
    ...(defaults?.putty_registry ? (["putty_registry"] as SourceKind[]) : []),
    "putty_reg",
    "moba_xterm",
    "secure_crt",
  ];

  return (
    <div className="dialog import-dialog">
      <h2>{t("import.title")}</h2>
      <label>
        {t("import.source")}
        <select value={kind} onChange={(e) => choose(e.target.value as SourceKind)}>
          {kinds.map((k) => (
            <option key={k} value={k}>
              {t(`import.kind.${k}`)}
            </option>
          ))}
        </select>
      </label>
      {needsPath && (
        <div className="row-form">
          <label>
            {t("import.path")}
            <input value={path} onChange={(e) => setPath(e.target.value)} />
          </label>
          <button type="button" onClick={() => void browse()}>
            {t("import.browse")}
          </button>
        </div>
      )}
      <div className="buttons">
        <button onClick={() => void load()} disabled={busy || (needsPath && !path.trim())}>
          {t("import.preview")}
        </button>
      </div>
      {error && <div className="error">{error}</div>}

      {keyFiles && (
        <div className="rows">
          {keyFiles.length === 0 && <p className="empty">{t("import.noKeys")}</p>}
          {keyFiles.map((file, i) => (
            <div key={file.path} className="card row">
              <label className="check">
                <input type="checkbox" checked={selected.has(i)} onChange={() => toggle(i)} />
                {file.name}
              </label>
              <span className="card-sub">{file.fingerprint ?? t("import.encrypted")}</span>
              {file.encrypted && selected.has(i) && (
                <input
                  type="password"
                  aria-label={t("import.passphrase", { name: file.name })}
                  placeholder={t("import.passphrase", { name: file.name })}
                  value={passphrases[file.path] ?? ""}
                  onChange={(e) => setPassphrases({ ...passphrases, [file.path]: e.target.value })}
                />
              )}
            </div>
          ))}
        </div>
      )}

      {preview && (
        <>
          {preview.plan.candidates.length === 0 && <p className="empty">{t("sshconfig.none")}</p>}
          <div className="rows">
            {preview.plan.candidates.map((c, i) => (
              <div key={i} className="card row">
                <label className="check">
                  <input type="checkbox" checked={selected.has(i)} onChange={() => toggle(i)} />
                  {c.name}
                </label>
                <span className="card-sub">{describe(c)}</span>
                {c.key_note && c.key_note.kind !== "file" && (
                  <span className="card-sub">{t(`sshconfig.keyNote.${c.key_note.kind}`)}</span>
                )}
                {c.key_note?.kind === "file" && (
                  <span className="card-sub">{t("import.keyFile", { path: c.key_note.path })}</span>
                )}
                {c.warnings.map((w, j) => (
                  <span key={j} className="card-sub warning">
                    {warning(w)}
                  </span>
                ))}
              </div>
            ))}
          </div>
          {preview.plan.skipped.length > 0 && (
            <div className="hint">
              <strong>{t("sshconfig.skipped.title")}</strong>
              <ul>
                {preview.plan.skipped.map((s, i) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
            </div>
          )}
          {kind === "known_hosts" && preview.host_keys !== null && (
            <label className="check">
              <input type="checkbox" checked={takeHostKeys} onChange={(e) => setTakeHostKeys(e.target.checked)} />
              {t("import.takeHostKeys", { count: preview.host_keys })}
            </label>
          )}
          {servers.length > 0 && (
            <label>
              {t("import.location")}
              <select value={location} onChange={(e) => setLocation(e.target.value)}>
                <option value="">{t("sync.localOnly")}</option>
                {servers.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
          )}
        </>
      )}

      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          onClick={() => void submit()}
          disabled={busy || (!preview && !keyFiles) || selected.size === 0}
        >
          {t("import.import")}
        </button>
      </div>
    </div>
  );
}
