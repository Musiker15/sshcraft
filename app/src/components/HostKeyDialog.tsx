import type { HostKeyPrompt } from "../api";
import { useT } from "../i18n/i18n";

type Props = { prompt: HostKeyPrompt; onAnswer: (accept: boolean) => void };

export function HostKeyDialog({ prompt, onAnswer }: Props) {
  const t = useT();
  return (
    <div className="overlay">
      <div className="dialog" role="alertdialog" aria-modal="true">
        <h2>{t("hostKey.title")}</h2>
        <p>{t("hostKey.text", { host: prompt.host_name, address: `${prompt.address}:${prompt.port}` })}</p>
        <div className="mono">{prompt.fingerprint}</div>
        <div className="buttons">
          {/* Focus on "reject": a stray Enter while typing elsewhere must never trust a key. */}
          <button onClick={() => onAnswer(false)} autoFocus>
            {t("hostKey.reject")}
          </button>
          <button className="primary" onClick={() => onAnswer(true)}>
            {t("hostKey.trust")}
          </button>
        </div>
      </div>
    </div>
  );
}
