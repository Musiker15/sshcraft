// "Run snippet": on new connections to hosts and/or in open panes, run (Enter) or only paste.
import { useState } from "react";
import type { Host, Snippet } from "../api";
import { useLanguage, useT } from "../i18n/i18n";
import { sortByName } from "../pages/hostsView";

export type OpenPane = { paneId: string; title: string; sessionId: string; hostId: string | null };
export type RunPlan = { hostIds: string[]; paneIds: string[]; run: boolean };

type Props = {
  snippet: Snippet;
  hosts: Host[];
  /** Panes with a session, in the order of the tabs. */
  panes: OpenPane[];
  onRun: (plan: RunPlan) => void;
  onCancel: () => void;
};

/** Toggles `id` in a list, keeping the order things were picked in. */
function toggle(list: string[], id: string): string[] {
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}

export function RunSnippetDialog({ snippet, hosts, panes, onRun, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const known = new Set(hosts.map((h) => h.id));
  const [hostIds, setHostIds] = useState(() => snippet.default_targets.filter((id) => known.has(id)));
  const [paneIds, setPaneIds] = useState(() =>
    panes.filter((p) => p.hostId !== null && snippet.default_targets.includes(p.hostId)).map((p) => p.paneId),
  );
  const [run, setRun] = useState(true);
  const nothing = hostIds.length === 0 && paneIds.length === 0;

  return (
    <div className="dialog run-snippet">
      <h2>{t("snippets.runTitle", { name: snippet.name })}</h2>
      <pre className="snippet-preview">{snippet.content}</pre>

      <fieldset className="choice">
        <legend>{t("snippets.mode")}</legend>
        <label className="check">
          <input type="radio" name="mode" checked={run} onChange={() => setRun(true)} />
          {t("snippets.modeRun")}
        </label>
        <label className="check">
          <input type="radio" name="mode" checked={!run} onChange={() => setRun(false)} />
          {t("snippets.modePaste")}
        </label>
      </fieldset>

      <h3 className="group-title">{t("snippets.newConnections")}</h3>
      <div className="check-list">
        {sortByName(hosts, lang).map((h) => (
          <label key={h.id} className="check">
            <input type="checkbox" checked={hostIds.includes(h.id)} onChange={() => setHostIds((l) => toggle(l, h.id))} />
            {h.name}
          </label>
        ))}
      </div>

      <h3 className="group-title">{t("snippets.openTabs")}</h3>
      {panes.length === 0 && <p className="hint">{t("snippets.noOpenTabs")}</p>}
      <div className="check-list">
        {panes.map((p) => (
          <label key={p.paneId} className="check">
            <input
              type="checkbox"
              checked={paneIds.includes(p.paneId)}
              onChange={() => setPaneIds((l) => toggle(l, p.paneId))}
            />
            {p.title}
          </label>
        ))}
      </div>

      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={nothing} onClick={() => onRun({ hostIds, paneIds, run })}>
          {t("snippets.run")}
        </button>
      </div>
    </div>
  );
}
