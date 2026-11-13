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
  sessionBookmark,
  sessionClose,
  sessionListDir,
  sessionOpen,
  sessionRecording,
  sessionResize,
  sessionWrite,
  snippetsList,
  type SessionStatus,
  type Snippet,
} from "../api";
import { CompletionPopup } from "./CompletionPopup";
import {
  currentWord,
  insertText,
  keyAction,
  keyGate,
  pathQuery,
  snippetSource,
  suggest,
  type Suggestion,
} from "../terminal/completion";
import { emptyLine, shouldRecord, track, type Line } from "../terminal/lineTracker";
import { HISTORY_CLEARED, SNIPPETS_CHANGED, pasteBytes, registerPane } from "../terminal/panes";
import { errorDetail } from "../i18n/errors";
import { useLanguage, useT } from "../i18n/i18n";
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

/** Height of one suggestion row and the list's padding, width of the list (as in the CSS). */
const POPUP_ROW = 20;
const POPUP_PAD = 10;
const POPUP_WIDTH = 320;

/**
 * Where the list goes: under the cursor in the pane, or above it when it would not fit below;
 * never past the pane's right edge. Measured on xterm's screen, so padding and scrollbar do not
 * shift it.
 */
function popupPosition(terminal: Terminal, element: HTMLElement, count: number): { left: number; top: number } {
  const screen = element.querySelector<HTMLElement>(".xterm-screen") ?? element;
  const pane = (element.offsetParent as HTMLElement | null) ?? element;
  const paneRect = pane.getBoundingClientRect();
  const rect = screen.getBoundingClientRect();
  const cellWidth = rect.width / Math.max(1, terminal.cols);
  const cellHeight = rect.height / Math.max(1, terminal.rows);
  const x = rect.left - paneRect.left + terminal.buffer.active.cursorX * cellWidth;
  const rowTop = rect.top - paneRect.top + terminal.buffer.active.cursorY * cellHeight;
  const height = count * POPUP_ROW + POPUP_PAD;
  const below = rowTop + cellHeight;
  const top = below + height > paneRect.height && rowTop - height >= 0 ? rowTop - height : below;
  const left = Math.max(0, Math.min(x, paneRect.width - POPUP_WIDTH));
  return { left, top };
}

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
  const t = useT();
  const lang = useLanguage();
  // Whether Rust records this session into a session log, and a short note after a bookmark.
  const [recording, setRecording] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  // Recording can stop on its own (size limit, storage failure): ask again now and then.
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => {
      const id = sessionId.current;
      if (id) sessionRecording(id).then(setRecording, () => {});
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [recording]);
  const noteTimer = useRef<number | undefined>(undefined);
  const showNote = (text: string) => {
    window.clearTimeout(noteTimer.current);
    setNote(text);
    noteTimer.current = window.setTimeout(() => setNote(null), 2000);
  };
  const bookmark = useRef(async () => {});
  bookmark.current = async () => {
    const id = sessionId.current;
    if (!id) return;
    try {
      await sessionBookmark(id, new Date().toLocaleTimeString(lang));
      showNote(t("terminal.bookmarkSet"));
    } catch {
      showNote(t("errors.not_recording"));
    }
  };
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
  const known = useRef({ history: [] as string[], snippets: [] as { name: string; content: string }[] });
  const listings = useRef(new Map<string, { at: number; names: string[] }>());
  const cwd = useRef<string | null>(pane.cwd ?? null);
  cwd.current = pane.cwd ?? null;
  const broadcasting = useRef(broadcast);
  broadcasting.current = broadcast;
  const listingTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const loadSnippets = () =>
      snippetsList().then(
        (list) => (known.current.snippets = (list ?? []).map((s: Snippet) => snippetSource(s))),
        () => {},
      );
    loadSnippets();
    if (scope)
      historyList(scope).then(
        (list) => (known.current.history = (list ?? []).map((e) => e.command)),
        () => {},
      );
    // A cleared history is not suggested any more; changed snippets are.
    const cleared = (e: Event) => {
      const which = (e as CustomEvent<string | null>).detail;
      if (which === null || which === scope) known.current.history = [];
    };
    window.addEventListener(HISTORY_CLEARED, cleared);
    window.addEventListener(SNIPPETS_CHANGED, loadSnippets);
    return () => {
      window.removeEventListener(HISTORY_CLEARED, cleared);
      window.removeEventListener(SNIPPETS_CHANGED, loadSnippets);
    };
  }, [scope]);

  /** Works out the suggestions for the line as it is now. */
  const refresh = useRef(() => {});
  refresh.current = () => {
    const terminal = term.current;
    const element = host.current;
    const text = line.current.text;
    const fullScreen = terminal?.buffer.active.type === "alternate";
    // Only at a shell prompt: never while a password is typed, nor in mysql, python and the like.
    const atShell = line.current.kind === "shell";
    if (!terminal || !element || !look.current.autocomplete || !line.current.certain || fullScreen || !atShell) {
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
          sessionListDir(id, query.dir, "").then(
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
    if (suggestions.length === 0) {
      if (shown.current.suggestions.length > 0) setCompletion(noCompletion);
      return;
    }
    setCompletion({ suggestions, selected: null, at: popupPosition(terminal, element, suggestions.length) });
  };

  /** Re-places an open list when the cursor moves (the echo of a key arrives after it). */
  const reposition = useRef(() => {});
  reposition.current = () => {
    const terminal = term.current;
    const element = host.current;
    const state = shown.current;
    if (!terminal || !element || state.suggestions.length === 0) return;
    setCompletion({ ...state, at: popupPosition(terminal, element, state.suggestions.length) });
  };

  /** Pastes into this pane's shell (bracketed where it allows) and follows it in the line. */
  const pasteInto = useRef<(content: string, run: boolean) => void>(() => {});
  pasteInto.current = (content, run) => {
    const bytes = pasteBytes(content, run, term.current?.modes.bracketedPasteMode ?? false);
    typed.current(bytes);
    callbacks.current.onInput(bytes, false);
  };

  /** Follows the keys sent to the shell: the line, and the history once Enter runs it. */
  const typed = useRef<(data: string) => void>(() => {});
  typed.current = (data: string) => {
    // The kind of the line (shell, secret, other) comes from the output before its first key.
    const kind = line.current.kind;
    const next = track(line.current, data, tail.current);
    const lineKind = line.current.kind ?? next.line.kind ?? kind;
    line.current = next.line;
    const command = next.committed;
    const terminal = term.current;
    if (command !== null && scope && look.current.history) {
      const context = {
        kind: lineKind,
        alternate: terminal?.buffer.active.type === "alternate",
        broadcast: broadcasting.current,
      };
      if (shouldRecord(command, context)) {
        // Suggested only once it is really kept (the vault may be locked).
        void historyRecord(scope, command).then(
          () => (known.current.history = [command, ...known.current.history.filter((c) => c !== command)]),
          () => {},
        );
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
    // Suggestions take keys only once one is selected; until then every key is the shell's. A
    // key they take is cancelled together with its keypress (else Enter would still run the line
    // and Tab move the focus).
    const suggestionKeys = keyGate((e) => {
      const state = shown.current;
      const open = state.suggestions.length > 0;
      const action = keyAction({ open, selected: state.selected }, e.key);
      const count = state.suggestions.length;
      switch (action) {
        case "pass":
          return false;
        case "close":
          setCompletion(noCompletion);
          // An unselected list closes, but Escape still reaches the shell.
          return state.selected !== null;
        case "select":
          setCompletion({ ...state, selected: 0 });
          return true;
        case "next":
          setCompletion({ ...state, selected: ((state.selected ?? 0) + 1) % count });
          return true;
        case "prev":
          setCompletion({ ...state, selected: ((state.selected ?? 0) - 1 + count) % count });
          return true;
        case "accept": {
          const chosen = state.suggestions[state.selected ?? 0];
          setCompletion(noCompletion);
          if (chosen) {
            const length = [...line.current.text].length;
            if (chosen.replace) {
              // Erase what is typed, then paste the snippet (several lines stay a paste).
              const erase = insertText({ ...chosen, insert: "" }, length);
              typed.current(erase);
              callbacks.current.onInput(erase, false);
              pasteInto.current(chosen.insert, false);
            } else {
              const text = insertText(chosen, length);
              typed.current(text);
              callbacks.current.onInput(text, false);
            }
          }
          return true;
        }
      }
    });
    terminal.attachCustomKeyEventHandler((e) => {
      if (matchShortcut(e, "terminal", resolveBindings(look.current.shortcuts)) !== null) return false;
      return suggestionKeys(e);
    });
    const cursor = terminal.onCursorMove(() => reposition.current());
    const unregister = registerPane(pane.id, {
      track: (data) => typed.current(data),
      paste: (content, run) => pasteInto.current(content, run),
      bookmark: () => void bookmark.current(),
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
        if (status.state === "ended") setRecording(false);
        // A template types its start once, into this pane only.
        const start = startup.current;
        if (status.state === "connected" && !start.typed && sessionId.current) {
          start.typed = true;
          const input = startupInput(start.target, start.startup);
          if (input) {
            typed.current(input);
            void sessionWrite(sessionId.current, input).catch(() => {});
          }
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
        sessionRecording(id).then(
          (on) => {
            if (!disposed) setRecording(on);
          },
          () => {},
        );
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
      cursor.dispose();
      unregister();
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
      if (input) {
        typed.current(input);
        void sessionWrite(pane.sessionId, input).catch(() => {});
      }
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
      {(recording || note) && (
        <span className="recording-badge" role="status" title={recording ? t("terminal.recording") : undefined}>
          {recording && <span className="recording-dot" aria-hidden="true" />}
          {note ?? t("terminal.rec")}
        </span>
      )}
      {focused && visible && (
        <CompletionPopup suggestions={completion.suggestions} selected={completion.selected} at={completion.at} />
      )}
    </>
  );
}
