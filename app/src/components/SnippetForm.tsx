// New or changed snippet: name, package, content and the hosts it usually runs on.
import { useState } from "react";
import { snippetSave, type Host, type Snippet } from "../api";
import { useLanguage, useT } from "../i18n/i18n";
import { sortByName } from "../pages/hostsView";
import { ErrorMessage } from "../shell/ErrorMessage";

type Props = {
  snippet?: Snippet;
  /** Packages that exist, offered while typing. */
  packages: string[];
  /** Package the new snippet starts in. */
  package?: string;
  hosts: Host[];
  onDone: () => void;
  onCancel: () => void;
};

export function SnippetForm({ snippet, packages, package: startPackage = "", hosts, onDone, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [name, setName] = useState(snippet?.name ?? "");
  const [pkg, setPkg] = useState(snippet?.package ?? startPackage);
  const [content, setContent] = useState(snippet?.content ?? "");
  const [targets, setTargets] = useState<string[]>(snippet?.default_targets ?? []);
  const [problem, setProblem] = useState<unknown>(null);

  async function save() {
    try {
      await snippetSave({
        id: snippet?.id ?? crypto.randomUUID(),
        name: name.trim(),
        package: pkg.trim(),
        content,
        default_targets: targets,
      });
      onDone();
    } catch (e) {
      setProblem(e);
    }
  }

  return (
    <form
      className="dialog"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <h2>{snippet ? t("snippets.editTitle") : t("snippets.newTitle")}</h2>
      <label>
        {t("snippets.name")}
        <input value={name} maxLength={100} required onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        {t("snippets.package")}
        <input list="snippet-packages" value={pkg} maxLength={100} onChange={(e) => setPkg(e.target.value)} />
        <datalist id="snippet-packages">
          {packages.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
      </label>
      <label>
        {t("snippets.content")}
        <textarea
          className="mono"
          rows={8}
          maxLength={10000}
          value={content}
          spellCheck={false}
          onChange={(e) => setContent(e.target.value)}
        />
      </label>
      <fieldset className="choice">
        <legend>{t("snippets.defaultTargets")}</legend>
        {hosts.length === 0 && <p className="hint">{t("hosts.empty")}</p>}
        <div className="check-list">
          {sortByName(hosts, lang).map((h) => (
            <label key={h.id} className="check">
              <input
                type="checkbox"
                checked={targets.includes(h.id)}
                onChange={() => setTargets((l) => (l.includes(h.id) ? l.filter((x) => x !== h.id) : [...l, h.id]))}
              />
              {h.name}
            </label>
          ))}
        </div>
      </fieldset>
      {problem !== null && <ErrorMessage error={problem} />}
      <div className="buttons">
        <button type="button" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="submit" className="primary" disabled={name.trim() === "" || content.trim() === ""}>
          {t("common.save")}
        </button>
      </div>
    </form>
  );
}
