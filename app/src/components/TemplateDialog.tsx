// "Save as workspace template": name, and per pane its title, working directory and command.
import { useState } from "react";
import { saveWorkspaceTemplate, type SessionTarget, type TemplatePane, type WorkspaceTemplate } from "../api";
import { useT } from "../i18n/i18n";
import { isPicker, type Pane, type SessionTab } from "../state/tabs";
import { ErrorMessage } from "../shell/ErrorMessage";

const MAX_NAME = 100;

type Props = {
  tab: SessionTab;
  existing: WorkspaceTemplate[];
  onDone: (saved: WorkspaceTemplate) => void;
  onCancel: () => void;
};

type Draft = { title: string; cwd: string; command: string };

/** The target as a template keeps it: a local shell's start directory lives in the pane. */
function templateTarget(target: SessionTarget): SessionTarget {
  return target.kind === "local" ? { kind: "local", shell: target.shell } : target;
}

/** Where the shell is now (OSC 7), else where the template started it. */
function startDir(pane: Pane): string {
  return pane.cwd ?? pane.startup?.cwd ?? "";
}

const orNull = (text: string) => (text.trim() === "" ? null : text.trim());

export function TemplateDialog({ tab, existing, onDone, onCancel }: Props) {
  const t = useT();
  const panes = tab.panes.filter((p): p is Pane => !isPicker(p));
  const [name, setName] = useState(() =>
    (tab.name ?? panes.map((p) => p.title).join(" · ")).slice(0, MAX_NAME),
  );
  const [drafts, setDrafts] = useState<Draft[]>(() =>
    panes.map((p) => ({ title: p.title, cwd: startDir(p), command: p.startup?.command ?? "" })),
  );
  const [problem, setProblem] = useState<unknown>(null);
  const same = existing.find((e) => e.name === name.trim());

  const change = (index: number, patch: Partial<Draft>) =>
    setDrafts((all) => all.map((d, i) => (i === index ? { ...d, ...patch } : d)));

  async function save() {
    // Picker panes are not kept, so the grid may change: then its sizes start equal again.
    const complete = panes.length === tab.panes.length;
    const templatePanes: TemplatePane[] = panes.map((p, i) => ({
      target: templateTarget(p.target),
      title: drafts[i].title.trim() || p.title,
      cwd: p.target.kind === "serial" ? null : orNull(drafts[i].cwd),
      command: orNull(drafts[i].command),
    }));
    try {
      const saved = await saveWorkspaceTemplate({
        id: same?.id ?? crypto.randomUUID(),
        name: name.trim(),
        layout: tab.layout,
        col_sizes: complete ? tab.colSizes : [],
        row_sizes: complete ? tab.rowSizes : [],
        panes: templatePanes,
      });
      onDone(saved);
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
      <h2>{t("templates.title")}</h2>
      <p className="hint">{t("templates.hint")}</p>
      <label>
        {t("templates.name")}
        <input value={name} maxLength={MAX_NAME} required onChange={(e) => setName(e.target.value)} />
      </label>
      {same && <p className="hint">{t("templates.overwrites")}</p>}
      {panes.length === 0 && <p className="empty">{t("templates.noPanes")}</p>}
      {panes.map((pane, index) => (
        <fieldset key={pane.id} className="template-pane">
          <legend>{pane.title}</legend>
          <label>
            {t("templates.paneTitle")}
            <input value={drafts[index].title} onChange={(e) => change(index, { title: e.target.value })} />
          </label>
          {pane.target.kind !== "serial" && (
            <label>
              {t("templates.cwd")}
              <input value={drafts[index].cwd} onChange={(e) => change(index, { cwd: e.target.value })} />
            </label>
          )}
          <label>
            {t("templates.command")}
            <input value={drafts[index].command} onChange={(e) => change(index, { command: e.target.value })} />
          </label>
        </fieldset>
      ))}
      {problem !== null && <ErrorMessage error={problem} />}
      <div className="buttons">
        <button type="button" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="submit" className="primary" disabled={panes.length === 0 || name.trim() === ""}>
          {t("common.save")}
        </button>
      </div>
    </form>
  );
}
