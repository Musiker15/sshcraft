import type { AgentQuestion } from "../api";
import { useT } from "../i18n/i18n";

type Props = { question: AgentQuestion; onAnswer: (allow: boolean) => void };

/** Asks before an AI agent's action runs. Only this window can answer, never the agent. */
export function AgentConfirmDialog({ question, onAnswer }: Props) {
  const t = useT();
  return (
    <div className="overlay">
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="agent-confirm-title">
        <h2 id="agent-confirm-title">{t("agents.confirmTitle")}</h2>
        <p>{t("agents.confirmText", { agent: question.agent, action: t(`agents.action.${question.action}`) })}</p>
        {question.target !== "" && (
          <p>
            <strong>{t("agents.target")}:</strong> {question.target}
          </p>
        )}
        {question.detail !== "" && <pre className="mono agent-detail">{question.detail}</pre>}
        <div className="buttons">
          {/* Focus on "deny": a stray Enter must never allow an action. */}
          <button onClick={() => onAnswer(false)} autoFocus>
            {t("agents.deny")}
          </button>
          <button className="primary" onClick={() => onAnswer(true)}>
            {t("agents.allow")}
          </button>
        </div>
      </div>
    </div>
  );
}
