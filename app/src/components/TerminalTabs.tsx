import { useEffect, useState, type Dispatch } from "react";
import type { SessionStatus } from "../api";
import type { TabsAction, TabsState } from "../state/tabs";
import { TerminalView } from "./TerminalView";

type Props = { state: TabsState; dispatch: Dispatch<TabsAction> };

export function TerminalTabs({ state, dispatch }: Props) {
  if (state.tabs.length === 0) {
    return <div className="empty">Doppelklick auf einen Host öffnet eine Verbindung.</div>;
  }
  return (
    <>
      <div className="tabbar">
        {state.tabs.map((tab) => (
          <button
            key={tab.id}
            className={tab.id === state.activeId ? "tab active" : "tab"}
            onClick={() => dispatch({ type: "activate", id: tab.id })}
          >
            {tab.title}
            <span
              role="button"
              title="Tab schließen"
              onClick={(e) => {
                e.stopPropagation();
                dispatch({ type: "close", id: tab.id });
              }}
            >
              ✕
            </span>
          </button>
        ))}
      </div>
      <div className="terminal-area">
        {state.tabs.map((tab) => (
          <div
            key={tab.id}
            className="terminal-pane"
            style={{ visibility: tab.id === state.activeId ? "visible" : "hidden" }}
          >
            <TerminalView
              tab={tab}
              active={tab.id === state.activeId}
              onAttach={(sessionId) => dispatch({ type: "attach", id: tab.id, sessionId })}
              onStatus={(status) => dispatch({ type: "status", id: tab.id, status })}
            />
            <StatusLine status={tab.status} />
          </div>
        ))}
      </div>
    </>
  );
}

function StatusLine({ status }: { status: SessionStatus }) {
  const countdown = useCountdown(status.state === "reconnecting" ? status.delay_secs : null, status);
  switch (status.state) {
    case "connecting":
      return <div className="statusline">Verbinde …</div>;
    case "connected":
      return <div className="statusline">Verbunden</div>;
    case "reconnecting":
      return (
        <div className="statusline warning">
          Verbindung verloren, neuer Versuch {status.attempt} von 5 in {countdown} s
        </div>
      );
    case "ended":
      return <EndedLine reason={status.reason} detail={status.detail} />;
  }
}

function EndedLine({ reason, detail }: { reason: string; detail: string | null }) {
  switch (reason) {
    case "exited":
      return <div className="statusline">Sitzung beendet</div>;
    case "host_key_mismatch":
      return (
        <div className="statusline danger">
          {"Achtung: Der Server meldet sich mit einem anderen Schlüssel als beim letzten Mal. " +
            "Das kann eine Neuinstallation sein oder ein Angriff. Die Verbindung wurde abgebrochen.\n" +
            (detail ?? "") +
            "\nWenn du sicher bist, dass der neue Schlüssel stimmt: Host bearbeiten, Fingerprint vergessen."}
        </div>
      );
    case "host_key_rejected":
      return <div className="statusline danger">Server-Schlüssel abgelehnt, nicht verbunden.</div>;
    case "auth_failed":
      return <div className="statusline danger">Der Server hat den Schlüssel nicht angenommen.</div>;
    case "lost":
      return <div className="statusline danger">Verbindung verloren{detail ? `: ${detail}` : ""}</div>;
    default:
      return <div className="statusline danger">Fehler{detail ? `: ${detail}` : ""}</div>;
  }
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
