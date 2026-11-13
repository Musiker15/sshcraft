import { useEffect, useState } from "react";
import type { AgentQuestion } from "../api";
import { needsCareful, visibleText } from "../agents/visible";
import { useT } from "../i18n/i18n";

type Props = { question: AgentQuestion; onAnswer: (allow: boolean) => void };

/** How long the buttons wait after a question appears, so a click meant for the one before
 * cannot answer it. */
export const ANSWER_DELAY_MS = 750;

/** Asks before an AI agent's action runs. Only this window can answer, never the agent. */
export function AgentConfirmDialog({ question, onAnswer }: Props) {
  const t = useT();
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setReady(true), ANSWER_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);
  return (
    <div className="overlay">
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="agent-confirm-title">
        <h2 id="agent-confirm-title">{t("agents.confirmTitle")}</h2>
        <p>
          {t("agents.confirmText", {
            agent: visibleText(question.agent),
            action: t(`agents.action.${question.action}`),
          })}
        </p>
        {question.target !== "" && (
          <p>
            <strong>{t("agents.target")}:</strong> {visibleText(question.target)}
          </p>
        )}
        {question.detail !== "" && (
          <>
            {needsCareful(question.detail) && <p className="warning-text">{t("agents.readAll")}</p>}
            <pre className="mono agent-detail">{visibleText(question.detail)}</pre>
          </>
        )}
        <div className="buttons">
          {/* Focus on "deny": a stray Enter must never allow an action. */}
          <button onClick={() => onAnswer(false)} autoFocus disabled={!ready}>
            {t("agents.deny")}
          </button>
          <button className="primary" onClick={() => onAnswer(true)} disabled={!ready}>
            {t("agents.allow")}
          </button>
        </div>
      </div>
    </div>
  );
}
