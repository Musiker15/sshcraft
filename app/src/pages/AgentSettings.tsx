// AI agents in the settings: a level per kind of action, what is hidden from them, and the log
// of what they asked for.
import { useCallback, useEffect, useState } from "react";
import {
  AGENT_CATEGORIES,
  AGENT_DEFAULTS,
  agentLogClear,
  agentLogList,
  agentPolicyGet,
  agentSetHidden,
  agentSetLevel,
  type AgentCategory,
  type AgentLevel,
  type AgentLogEntry,
  type AgentPolicy,
  type Group,
  type Host,
} from "../api";
import { emptyPolicy } from "../agents/policy";
import { visibleText } from "../agents/visible";
import { useLanguage, useT } from "../i18n/i18n";
import { ErrorMessage } from "../shell/ErrorMessage";

type Props = { hosts: Host[]; groups: Group[] };

export function AgentSettings({ hosts, groups }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [policy, setPolicy] = useState<AgentPolicy>(emptyPolicy);
  const [log, setLog] = useState<AgentLogEntry[]>([]);
  const [problem, setProblem] = useState<unknown>(null);

  const loadLog = useCallback(() => {
    agentLogList().then((entries) => setLog(entries ?? []), () => setLog([]));
  }, []);
  useEffect(() => {
    agentPolicyGet().then((p) => setPolicy(p ?? emptyPolicy), setProblem);
    loadLog();
  }, [loadLog]);

  const apply = (change: Promise<AgentPolicy>) => change.then((saved) => saved && setPolicy(saved), setProblem);
  const level = (category: AgentCategory): AgentLevel => policy.levels[category] ?? AGENT_DEFAULTS[category];
  const name = (id: string) =>
    hosts.find((h) => h.id === id)?.name ?? groups.find((g) => g.id === id)?.name ?? t("agents.unknownItem");

  return (
    <div className="settings-block">
      <h2 className="group-title">{t("agents.title")}</h2>
      <p className="hint">{t("agents.hint")}</p>
      {problem !== null && <ErrorMessage error={problem} />}
      <div className="agent-levels">
        {AGENT_CATEGORIES.map((category) => (
          <label key={category}>
            {t(`agents.category.${category}`)}
            <select
              value={level(category)}
              onChange={(e) => void apply(agentSetLevel(category, e.target.value as AgentLevel))}
            >
              <option value="allow">{t("agents.level.allow")}</option>
              <option value="ask">{t("agents.level.ask")}</option>
              <option value="deny">{t("agents.level.deny")}</option>
            </select>
          </label>
        ))}
      </div>
      <h3>{t("agents.hidden")}</h3>
      {policy.hidden.length === 0 ? (
        <p className="hint">{t("agents.hiddenNone")}</p>
      ) : (
        <ul className="agent-hidden">
          {policy.hidden.map((id) => (
            <li key={id}>
              {name(id)}{" "}
              <button className="link" onClick={() => void apply(agentSetHidden(id, false))}>
                {t("agents.show")}
              </button>
            </li>
          ))}
        </ul>
      )}
      <h3>{t("agents.log")}</h3>
      {log.length === 0 ? (
        <p className="hint">{t("agents.logEmpty")}</p>
      ) : (
        <>
          <table className="agent-log">
            <tbody>
              {log.map((entry) => (
                <tr key={entry.id}>
                  <td>{new Date(entry.at).toLocaleString(lang)}</td>
                  <td>{entry.agent}</td>
                  <td>{t(`agents.action.${entry.action}`)}</td>
                  <td>{entry.target}</td>
                  <td className="mono">{visibleText(entry.detail)}</td>
                  <td>{t(`agents.outcome.${entry.outcome}`)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <button onClick={() => void agentLogClear().then(loadLog, setProblem)}>{t("agents.logClear")}</button>
        </>
      )}
    </div>
  );
}
