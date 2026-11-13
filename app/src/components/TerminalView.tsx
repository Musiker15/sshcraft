import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { openLink, sessionAck, sessionClose, sessionOpen, sessionResize, sessionWrite, type SessionStatus } from "../api";
import { errorDetail } from "../i18n/errors";
import { useSettings } from "../settings/SettingsProvider";
import { terminalTheme } from "../settings/themes";
import { syncTerminalFocus } from "../shell/focus";
import { matchShortcut, resolveBindings } from "../shell/shortcuts";
import type { Activity, Pane } from "../state/tabs";
import { expectsInput, isOpenableLink, isTerminalReport, osc52Text, osc7Path, startupInput } from "../terminal/features";

/** How much recent output is kept to see whether the pane waits for input. */
const TAIL = 200;
/** Output reports its dot at most this often. */
const ACTIVITY_MS = 250;

type Props = {
  pane: Pane;
  /** On screen (its tab is active and, in focus layout, it is the main pane). */
  visible: boolean;
  /** Has the keyboard. */
  focused: boolean;
  onAttach: (sessionId: string) => void;
  onStatus: (status: SessionStatus) => void;
  /** Typed input; `report` marks the terminal answering the server (never broadcast). */
  onInput: (data: string, report: boolean) => void;
  onActivity: (activity: Activity) => void;
  onCwd: (cwd: string) => void;
  onFocus: () => void;
};

/** Opens a link only on Ctrl+click and only for web addresses (Rust checks again). */
function openOnCtrlClick(event: MouseEvent, uri: string) {
  if (event.ctrlKey && isOpenableLink(uri)) void openLink(uri).catch(() => {});
}

/** One xterm.js terminal bound to one session for the lifetime of its pane. */
export function TerminalView({ pane, visible, focused, onAttach, onStatus, onInput, onActivity, onCwd, onFocus }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const fit = useRef<FitAddon | null>(null);
  const term = useRef<Terminal | null>(null);
  const { settings } = useSettings();
  const look = useRef(settings);
  look.current = settings;
  const sessionId = useRef<string | null>(null);
  const callbacks = useRef({ onAttach, onStatus, onInput, onActivity, onCwd, onFocus });
  callbacks.current = { onAttach, onStatus, onInput, onActivity, onCwd, onFocus };
  const startup = useRef({ target: pane.target, startup: pane.startup, typed: false });

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: look.current.font_family,
      fontSize: look.current.font_size,
      theme: terminalTheme(look.current.terminal_theme).colors,
      scrollback: 10000,
      // OSC 8 hyperlinks follow the same rule as plain links.
      linkHandler: { activate: openOnCtrlClick, allowNonHttpProtocols: false },
    });
    term.current = terminal;
    // Window shortcuts are handled by the shell; xterm must not also send them to the server.
    terminal.attachCustomKeyEventHandler(
      (e) => matchShortcut(e, "terminal", resolveBindings(look.current.shortcuts)) === null,
    );
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon(openOnCtrlClick));
    // OSC 52: a program may put text on the clipboard (if allowed), never read it.
    // xterm does not catch what a parser hook throws: one exception would stop this terminal for
    // good (and with it the flow control). Every hook swallows its own errors.
    const clipboard = terminal.parser.registerOscHandler(52, (data) => {
      try {
        if (look.current.clipboard_osc52 === "write") {
          const text = osc52Text(data);
          if (text !== null) void navigator.clipboard.writeText(text).catch(() => {});
        }
      } catch {
        // Ignored: the sequence just does nothing.
      }
      return true;
    });
    const directory = terminal.parser.registerOscHandler(7, (data) => {
      try {
        const path = osc7Path(data);
        if (path) callbacks.current.onCwd(path);
      } catch {
        // Ignored: the sequence just does nothing.
      }
      return true;
    });
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

    // The last bit of output decides between "new output" and "waits for input".
    const decoder = new TextDecoder();
    let tail = "";
    let pending: ReturnType<typeof setTimeout> | null = null;
    const noteOutput = (bytes: Uint8Array) => {
      tail = (tail + decoder.decode(bytes, { stream: true })).slice(-TAIL);
      pending ??= setTimeout(() => {
        pending = null;
        callbacks.current.onActivity(expectsInput(tail) ? "input" : "output");
      }, ACTIVITY_MS);
    };
    const bell = terminal.onBell(() => callbacks.current.onActivity("input"));

    sessionOpen(
      pane.target,
      terminal.cols,
      terminal.rows,
      // Acknowledge only after xterm has rendered, so Rust pauses reading while the UI is behind.
      (bytes) => {
        terminal.write(bytes, () => rendered(bytes.length));
        noteOutput(bytes);
      },
      (status) => {
        callbacks.current.onStatus(status);
        // A template types its start once, into this pane only.
        const start = startup.current;
        if (status.state === "connected" && !start.typed && sessionId.current) {
          start.typed = true;
          const input = startupInput(start.target, start.startup);
          if (input) void sessionWrite(sessionId.current, input).catch(() => {});
        }
      },
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

    const input = terminal.onData((data) => callbacks.current.onInput(data, isTerminalReport(data)));
    const resized = terminal.onResize(({ cols, rows }) => {
      if (sessionId.current) void sessionResize(sessionId.current, cols, rows).catch(() => {});
    });
    const observer = new ResizeObserver(() => {
      if (element.offsetWidth > 0 && element.offsetHeight > 0) fitAddon.fit();
    });
    observer.observe(element);
    const focusIn = () => callbacks.current.onFocus();
    element.addEventListener("focusin", focusIn);

    return () => {
      disposed = true;
      if (pending) clearTimeout(pending);
      element.removeEventListener("focusin", focusIn);
      observer.disconnect();
      input.dispose();
      resized.dispose();
      bell.dispose();
      clipboard.dispose();
      directory.dispose();
      if (sessionId.current) void sessionClose(sessionId.current);
      terminal.dispose();
    };
    // The session belongs to the pane; it is opened once per mounted pane.
  }, [pane.id]);

  // A connected session whose first status came before the id: type the start now.
  useEffect(() => {
    const start = startup.current;
    if (pane.sessionId && pane.status.state === "connected" && !start.typed) {
      start.typed = true;
      const input = startupInput(start.target, start.startup);
      if (input) void sessionWrite(pane.sessionId, input).catch(() => {});
    }
  }, [pane.sessionId, pane.status.state]);

  useEffect(() => {
    if (visible && host.current && host.current.offsetWidth > 0) fit.current?.fit();
    if (term.current) syncTerminalFocus(term.current, visible && focused);
  }, [visible, focused]);

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
