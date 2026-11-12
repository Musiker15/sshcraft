import { useEffect, useState } from "react";
import type { EndReason, SessionStatus } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

/** The line under a terminal: connecting, connected, reconnecting or why it ended. */
export function SessionStatusLine({ status }: { status: SessionStatus }) {
  const t = useT();
  const countdown = useCountdown(status.state === "reconnecting" ? status.delay_secs : null, status);
  switch (status.state) {
    case "connecting":
      return <div className="statusline">{t("session.status.connecting")}</div>;
    case "connected":
      return <div className="statusline">{t("session.status.connected")}</div>;
    case "reconnecting":
      return (
        <div className="statusline warning">
          {t("session.status.reconnecting", { attempt: status.attempt, seconds: countdown })}
        </div>
      );
    case "ended":
      return <EndedLine reason={status.reason} code={status.code} detail={status.detail} />;
  }
}

export function EndedLine({ reason, code, detail }: { reason: EndReason; code: string | null; detail: string | null }) {
  const t = useT();
  const headline = t(`session.ended.${reason}`);
  // For these reasons the code says it all; elsewhere it explains the headline.
  const text = !code
    ? headline
    : reason === "error" || reason === "auth_failed"
      ? errorText(t, { code, detail })
      : `${headline}: ${errorText(t, { code, detail })}`;
  return (
    <div className={reason === "exited" ? "statusline" : "statusline danger"}>
      <span>{text}</span>
      {detail && <code className="detail">{detail}</code>}
    </div>
  );
}

function useCountdown(seconds: number | null, key: unknown): number {
  const [left, setLeft] = useState(seconds ?? 0);
  useEffect(() => {
    if (seconds === null) return;
    setLeft(seconds);
    const timer = setInterval(() => setLeft((s) => Math.max(0, s - 1)), 1000);
    return () => clearInterval(timer);
  }, [seconds, key]);
  return left;
}
