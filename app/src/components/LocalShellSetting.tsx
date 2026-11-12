import { useEffect, useState } from "react";
import { localShellGet, localShellSet } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

/** The user's own shell for local terminals: program and arguments (one per line). */
export function LocalShellSetting() {
  const t = useT();
  const [program, setProgram] = useState("");
  const [args, setArgs] = useState("");
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    localShellGet().then(
      (shell) => {
        setProgram(shell?.program ?? "");
        setArgs(shell?.args.join("\n") ?? "");
      },
      () => {},
    );
  }, []);

  async function save() {
    const trimmed = program.trim();
    try {
      await localShellSet(
        trimmed === ""
          ? null
          : {
              program: trimmed,
              args: args.split("\n").map((a) => a.trim()).filter((a) => a !== ""),
              label: trimmed.split(/[\\/]/).pop() ?? trimmed,
            },
      );
      setNote(t("settings.saved"));
    } catch (e) {
      setNote(errorText(t, e));
    }
  }

  return (
    <div className="settings-block">
      <h2 className="group-title">{t("settings.localShell")}</h2>
      <p className="hint">{t("settings.localShellHint")}</p>
      <label>
        {t("settings.localShellProgram")}
        <input value={program} onChange={(e) => setProgram(e.target.value)} />
      </label>
      <label>
        {t("settings.localShellArgs")}
        <textarea value={args} onChange={(e) => setArgs(e.target.value)} />
      </label>
      <div className="buttons">
        <button onClick={save}>{t("common.save")}</button>
        {note && <span className="hint">{note}</span>}
      </div>
    </div>
  );
}
