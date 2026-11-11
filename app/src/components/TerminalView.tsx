import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { sessionAck, sessionClose, sessionOpen, sessionResize, sessionWrite, type SessionStatus } from "../api";
import type { Tab } from "../state/tabs";

type Props = {
  tab: Tab;
  active: boolean;
  onAttach: (sessionId: string) => void;
  onStatus: (status: SessionStatus) => void;
};

/** One xterm.js terminal bound to one session for the lifetime of its tab. */
export function TerminalView({ tab, active, onAttach, onStatus }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const fit = useRef<FitAddon | null>(null);
  const sessionId = useRef<string | null>(null);
  const callbacks = useRef({ onAttach, onStatus });
  callbacks.current = { onAttach, onStatus };

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: "Cascadia Mono, Consolas, monospace",
      fontSize: 14,
      theme: { background: "#0d0f12" },
      scrollback: 10000,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(element);
    fitAddon.fit();
    fit.current = fitAddon;
    let disposed = false;
    // Output can arrive before sessionOpen resolves; acknowledge it once the id is known.
    let unacked = 0;
    const rendered = (bytes: number) => {
      if (sessionId.current) void sessionAck(sessionId.current, bytes).catch(() => {});
      else unacked += bytes;
    };

    sessionOpen(
      tab.hostId,
      terminal.cols,
      terminal.rows,
      // Acknowledge only after xterm has rendered, so Rust pauses reading while the UI is behind.
      (bytes) => terminal.write(bytes, () => rendered(bytes.length)),
      (status) => callbacks.current.onStatus(status),
    ).then(
      (id) => {
        if (disposed) {
          void sessionClose(id);
          return;
        }
        sessionId.current = id;
        callbacks.current.onAttach(id);
        if (unacked > 0) {
          void sessionAck(id, unacked).catch(() => {});
          unacked = 0;
        }
      },
      (e) => callbacks.current.onStatus({ state: "ended", reason: "error", detail: String(e?.message ?? e) }),
    );

    const input = terminal.onData((data) => {
      if (sessionId.current) void sessionWrite(sessionId.current, data).catch(() => {});
    });
    const resized = terminal.onResize(({ cols, rows }) => {
      if (sessionId.current) void sessionResize(sessionId.current, cols, rows).catch(() => {});
    });
    const observer = new ResizeObserver(() => {
      if (element.offsetWidth > 0 && element.offsetHeight > 0) fitAddon.fit();
    });
    observer.observe(element);

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      resized.dispose();
      if (sessionId.current) void sessionClose(sessionId.current);
      terminal.dispose();
    };
    // The session belongs to the tab; it is opened once per mounted tab.
  }, [tab.id]);

  useEffect(() => {
    if (active && host.current && host.current.offsetWidth > 0) fit.current?.fit();
  }, [active]);

  return <div className="terminal-host" ref={host} />;
}
