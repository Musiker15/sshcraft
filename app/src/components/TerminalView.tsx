import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  historyList,
  historyRecord,
  openLink,
  sessionAck,
  sessionClose,
  sessionListDir,
  sessionOpen,
  sessionResize,
  sessionWrite,
  snippetsList,
  type SessionStatus,
  type Snippet,
} from "../api";
import { CompletionPopup } from "./CompletionPopup";
import { currentWord, insertText, keyAction, pathQuery, suggest, type Suggestion } from "../terminal/completion";
import { emptyLine, shouldRecord, track, type Line } from "../terminal/lineTracker";
import { errorDetail } from "../i18n/errors";
import { useSettings } from "../settings/SettingsProvider";
import { terminalTheme } from "../settings/themes";
import { syncTerminalFocus } from "../shell/focus";
import { matchShortcut, resolveBindings } from "../shell/shortcuts";
import type { Activity, Pane } from "../state/tabs";
import {
  expectsInput,
  historyScope,
  isOpenableLink,
  isTerminalReport,
  osc52Text,
  osc7Path,
  startupInput,
} from "../terminal/features";

/** A directory listing is used this long before it is asked for again. */
const LISTING_TTL_MS = 10_000;
/** Typing pauses this long before a directory is listed. */
const LISTING_DELAY_MS = 250;

type Completion = { suggestions: Suggestion[]; selected: number | null; at: { left: number; top: number } };
const noCompletion: Completion = { suggestions: [], selected: null, at: { left: 0, top: 0 } };

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
  /** Keys go to every pane of the tab: nothing typed is kept in the history then. */
  broadcast?: boolean;
};

/** Opens a link only on Ctrl+click and only for web addresses (Rust checks again). */
function openOnCtrlClick(event: MouseEvent, uri: string) {
  if (event.ctrlKey && isOpenableLink(uri)) void openLink(uri).catch(() => {});
}

/** One xterm.js terminal bound to one session for the lifetime of its pane. */
export function TerminalView({
  pane,
  visible,
  focused,
  onAttach,
  onStatus,
  onInput,
  onActivity,
  onCwd,
  onFocus,
  broadcast = false,
}: Props) {
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

  // Completion and history: the typed line, where the shell is, what can be suggested.
  const [completion, setCompletion] = useState<Completion>(noCompletion);
  const shown = useRef(completion);
  shown.current = completion;
  const line = useRef<Line>(emptyLine);
  const tail = useRef("");
  const scope = historyScope(pane.target);
  const known = useRef({ history: [] as string[], snippets: [] as Snippet[] });
  const listings = useRef(new Map<string, { at: number; names: string[] }>());
  const cwd = useRef<string | null>(pane.cwd ?? null);
  cwd.current = pane.cwd ?? null;
  const broadcasting = useRef(broadcast);
  broadcasting.current = broadcast;
  const listingTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    snippetsList().then((list) => (known.current.snippets = list ?? []), () => {});
    if (scope)
      historyList(scope).then(
        (list) => (known.current.history = (list ?? []).map((e) => e.command)),
        () => {},
      );
  }, [scope]);

  /** Works out the suggestions for the line as it is now. */
  const refresh = useRef(() => {});
  refresh.current = () => {
    const terminal = term.current;
    const element = host.current;
    const text = line.current.text;
    const fullScreen = terminal?.buffer.active.type === "alternate";
    if (!terminal || !element || !look.current.autocomplete || !line.current.certain || fullScreen) {
      if (shown.current.suggestions.length > 0) setCompletion(noCompletion);
      return;
    }
    // Paths: from a fresh enough listing, else ask once typing pauses.
    let paths: string[] | null = null;
    const query = pathQuery(currentWord(text).word, cwd.current);
    if (query && sessionId.current && pane.target.kind !== "serial") {
      const cached = listings.current.get(query.dir);
      if (cached && Date.now() - cached.at < LISTING_TTL_MS) {
        paths = cached.names;
      } else {
        if (listingTimer.current) clearTimeout(listingTimer.current);
        const id = sessionId.current;
        listingTimer.current = setTimeout(() => {
          sessionListDir(id, query.dir).then(
            (names) => {
              listings.current.set(query.dir, { at: Date.now(), names: names ?? [] });
              refresh.current();
            },
            () => listings.current.set(query.dir, { at: Date.now(), names: [] }),
          );
        }, LISTING_DELAY_MS);
      }
    }
    const suggestions = suggest(text, { history: known.current.history, snippets: known.current.snippets, paths });
    // Under the cursor, inside the pane.
    const cellWidth = element.clientWidth / Math.max(1, terminal.cols);
    const cellHeight = element.clientHeight / Math.max(1, terminal.rows);
    const at = {
      left: element.offsetLeft + terminal.buffer.active.cursorX * cellWidth,
      top: element.offsetTop + (terminal.buffer.active.cursorY + 1) * cellHeight,
    };
    setCompletion({ suggestions, selected: null, at });
  };

  /** Follows the keys sent to the shell: the line, and the history once Enter runs it. */
  const typed = useRef<(data: string) => void>(() => {});
  typed.current = (data: string) => {
    // Asked before the keys change what is on screen: no echo after "Password:".
    const secretPrompt = expectsInput(tail.current);
    const next = track(line.current, data);
    line.current = next.line;
    const command = next.committed;
    const terminal = term.current;
    if (command !== null && scope && look.current.history) {
      const context = {
        secretPrompt,
        alternate: terminal?.buffer.active.type === "alternate",
        broadcast: broadcasting.current,
      };
      if (shouldRecord(command, context)) {
        known.current.history = [command, ...known.current.history.filter((c) => c !== command)];
        void historyRecord(scope, command).catch(() => {});
      }
    }
    refresh.current();
  };

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
    terminal.attachCustomKeyEventHandler((e) => {
      if (matchShortcut(e, "terminal", resolveBindings(look.current.shortcuts)) !== null) return false;
      if (e.type !== "keydown") return true;
      // Suggestions take keys only once one is selected; until then every key is the shell's.
      const state = shown.current;
      const open = state.suggestions.length > 0;
      const action = keyAction({ open, selected: state.selected }, e.key);
      const count = state.suggestions.length;
      switch (action) {
        case "pass":
          return true;
        case "close":
          setCompletion(noCompletion);
          // An unselected list closes, but Escape still reaches the shell.
          return state.selected === null;
        case "select":
          setCompletion({ ...state, selected: 0 });
          return false;
        case "next":
          setCompletion({ ...state, selected: ((state.selected ?? 0) + 1) % count });
          return false;
        case "prev":
          setCompletion({ ...state, selected: ((state.selected ?? 0) - 1 + count) % count });
          return false;
        case "accept": {
          const chosen = state.suggestions[state.selected ?? 0];
          setCompletion(noCompletion);
          if (chosen) {
            const text = insertText(chosen);
            callbacks.current.onInput(text, false);
            line.current = track(line.current, text).line;
          }
          return false;
        }
      }
    });
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
    // `tail` (a ref) keeps the last output: it decides "waits for input" and password prompts.
    let pending: ReturnType<typeof setTimeout> | null = null;
    const noteOutput = (bytes: Uint8Array) => {
      tail.current = (tail.current + decoder.decode(bytes, { stream: true })).slice(-TAIL);
      pending ??= setTimeout(() => {
        pending = null;
        callbacks.current.onActivity(expectsInput(tail.current) ? "input" : "output");
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

    const input = terminal.onData((data) => {
      const report = isTerminalReport(data);
      if (!report) typed.current(data);
      callbacks.current.onInput(data, report);
    });
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
      if (listingTimer.current) clearTimeout(listingTimer.current);
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

  return (
    <>
      <div className="terminal-host" ref={host} />
      {focused && visible && (
        <CompletionPopup suggestions={completion.suggestions} selected={completion.selected} at={completion.at} />
      )}
    </>
  );
}
