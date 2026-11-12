import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { sessionAck, sessionClose, sessionOpen, sessionResize, sessionWrite, type SessionStatus } from "../api";
import { errorDetail } from "../i18n/errors";
import { useSettings } from "../settings/SettingsProvider";
import { terminalTheme } from "../settings/themes";
import { syncTerminalFocus } from "../shell/focus";
import { matchShortcut } from "../shell/shortcuts";
import type { SessionTab } from "../state/tabs";

type Props = {
  tab: SessionTab;
  active: boolean;
  onAttach: (sessionId: string) => void;
  onStatus: (status: SessionStatus) => void;
};

/** One xterm.js terminal bound to one session for the lifetime of its tab. */
export function TerminalView({ tab, active, onAttach, onStatus }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const fit = useRef<FitAddon | null>(null);
  const term = useRef<Terminal | null>(null);
  const { settings } = useSettings();
  const look = useRef(settings);
  look.current = settings;
  const sessionId = useRef<string | null>(null);
  const callbacks = useRef({ onAttach, onStatus });
  callbacks.current = { onAttach, onStatus };

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: look.current.font_family,
      fontSize: look.current.font_size,
      theme: terminalTheme(look.current.terminal_theme).colors,
      scrollback: 10000,
    });
    term.current = terminal;
    // Window shortcuts are handled by the shell; xterm must not also send them to the server.
    terminal.attachCustomKeyEventHandler((e) => matchShortcut(e, "terminal") === null);
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
      (e) =>
        callbacks.current.onStatus({
          state: "ended",
          reason: "error",
          code: typeof e?.code === "string" ? e.code : null,
          detail: errorDetail(e),
        }),
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
    if (term.current) syncTerminalFocus(term.current, active);
  }, [active]);

  // Font and colours follow the settings while the session keeps running.
  useEffect(() => {
    const terminal = term.current;
    if (!terminal) return;
    terminal.options.fontFamily = settings.font_family;
    terminal.options.fontSize = settings.font_size;
    terminal.options.theme = terminalTheme(settings.terminal_theme).colors;
    if (host.current && host.current.offsetWidth > 0) fit.current?.fit();
  }, [settings.font_family, settings.font_size, settings.terminal_theme]);

  return <div className="terminal-host" ref={host} />;
}
