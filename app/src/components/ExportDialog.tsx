import { save } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { exportHosts, type ExportFilter, type Group, type ServerView } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { groupLabel } from "../pages/hostsTree";

type Props = { groups: Group[]; servers: ServerView[]; onDone: () => void };

/** Hosts and groups as CSV or JSON, without keys or passwords. */
export function ExportDialog({ groups, servers, onDone }: Props) {
  const t = useT();
  const [format, setFormat] = useState<"csv" | "json">("csv");
  const [scope, setScope] = useState("all");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  function filter(): ExportFilter {
    if (scope === "local") return { location: { kind: "local" } };
    if (scope.startsWith("server:")) return { location: { kind: "server", id: scope.slice(7) } };
    if (scope.startsWith("group:")) return { group: scope.slice(6) };
    return {};
  }

  async function run() {
    setError(null);
    try {
      const path = await save({
        defaultPath: `sshcraft-hosts.${format}`,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      });
      if (!path) return;
      await exportHosts(format, filter(), path);
      setSaved(true);
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  return (
    <div className="dialog">
      <h2>{t("export.title")}</h2>
      <p className="hint">{t("export.noSecrets")}</p>
      <fieldset className="choices">
        {(["csv", "json"] as const).map((f) => (
          <label key={f} className="check">
            <input type="radio" name="format" checked={format === f} onChange={() => setFormat(f)} />
            {t(`export.format.${f}`)}
          </label>
        ))}
      </fieldset>
      <label>
        {t("export.scope")}
        <select value={scope} onChange={(e) => setScope(e.target.value)}>
          <option value="all">{t("export.all")}</option>
          <option value="local">{t("sync.localOnly")}</option>
          {servers.map((s) => (
            <option key={s.id} value={`server:${s.id}`}>
              {s.name}
            </option>
          ))}
          {groups.map((g) => (
            <option key={g.id} value={`group:${g.id}`}>
              {groupLabel(groups, g.id)}
            </option>
          ))}
        </select>
      </label>
      {saved && <p>{t("export.saved")}</p>}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onDone}>{t("common.close")}</button>
        <button className="primary" onClick={() => void run()}>
          {t("export.save")}
        </button>
      </div>
    </div>
  );
}
