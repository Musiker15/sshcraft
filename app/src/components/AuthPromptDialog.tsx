import { useState } from "react";
import type { AuthPrompt } from "../api";
import { useT } from "../i18n/i18n";

type Props = { prompt: AuthPrompt; onAnswer: (answers: string[] | null) => void };

/** The server's sign-in questions (a password, a one-time code), or SSHCraft's question for a
 * key's passphrase. Answers are not kept. */
export function AuthPromptDialog({ prompt, onAnswer }: Props) {
  const t = useT();
  const [answers, setAnswers] = useState<string[]>(prompt.prompts.map(() => ""));
  return (
    <div className="overlay">
      <form
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        onSubmit={(e) => {
          e.preventDefault();
          onAnswer(answers);
        }}
      >
        {prompt.passphrase_for ? (
          <>
            <h2>{t("auth.passphraseTitle", { name: prompt.passphrase_for })}</h2>
            <p>{t("auth.passphraseText", { host: prompt.host_name })}</p>
          </>
        ) : (
          <>
            <h2>{prompt.name || t("auth.title")}</h2>
            <p>{t("auth.text", { host: prompt.host_name, user: prompt.username })}</p>
          </>
        )}
        {prompt.instructions && <p className="hint">{prompt.instructions}</p>}
        {prompt.prompts.map((line, index) => (
          <label key={index}>
            {line.text}
            <input
              type={line.echo ? "text" : "password"}
              autoComplete="off"
              autoFocus={index === 0}
              value={answers[index]}
              onChange={(e) => setAnswers(answers.map((a, i) => (i === index ? e.target.value : a)))}
            />
          </label>
        ))}
        <div className="buttons">
          <button type="button" onClick={() => onAnswer(null)}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("auth.submit")}
          </button>
        </div>
      </form>
    </div>
  );
}
