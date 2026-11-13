// Tabs of the window: the start page (always first), new-tab pages and session tabs. A session
// tab is a workspace of 1 to 16 panes, each pane one session (or a picker that becomes one).
// Panes keep their id wherever they move, so their terminal and connection stay.
import type { SessionStatus, SessionTarget } from "../api";

export const HOME_ID = "home";
export const MAX_PANES = 16;

/** Status dot of a pane, from least to most urgent. */
export type Activity = "none" | "output" | "input" | "error";
const URGENCY: Record<Activity, number> = { none: 0, output: 1, input: 2, error: 3 };

export type Startup = {
  cwd?: string | null;
  command?: string | null;
  /** Typed as it is instead of `cd` and command (a snippet run in new connections). */
  raw?: string;
};

export type Pane = {
  id: string;
  target: SessionTarget;
  title: string;
  sessionId?: string;
  status: SessionStatus;
  activity: Activity;
  /** Typed once after the first connect (templates). */
  startup?: Startup;
  /** The working directory the shell last reported (OSC 7). */
  cwd?: string;
};
export type PickerPane = { id: string; picker: true };
export type AnyPane = Pane | PickerPane;
export type Layout = "split" | "focus";

export type HomeTab = { kind: "home"; id: typeof HOME_ID };
export type NewTab = { kind: "new"; id: string };
export type SessionTab = {
  kind: "session";
  id: string;
  panes: AnyPane[];
  focusedId: string;
  layout: Layout;
  broadcast: boolean;
  /** Shares of the split grid; empty or not fitting means equal. */
  colSizes: number[];
  rowSizes: number[];
  /** Set when opened from a template. */
  name?: string;
};
export type Tab = HomeTab | NewTab | SessionTab;

export type TabsState = { tabs: Tab[]; activeId: string };

type Target = { target: SessionTarget; title: string };

export type TabsAction =
  | { type: "open"; tab: { id: string; startup?: Startup } & Target }
  | { type: "openNew"; id: string }
  | ({ type: "pick"; id: string } & Target)
  | ({ type: "pickPane"; paneId: string } & Target)
  | { type: "attach"; paneId: string; sessionId: string }
  | { type: "status"; paneId: string; status: SessionStatus }
  | { type: "activity"; paneId: string; activity: Activity }
  | { type: "cwd"; paneId: string; cwd: string }
  | { type: "close"; id: string }
  | { type: "activate"; id: string }
  | { type: "next" }
  | { type: "prev" }
  | { type: "split"; tabId: string; paneId: string }
  | { type: "focusPane"; paneId: string }
  | { type: "closePane"; paneId: string }
  | { type: "detachPane"; paneId: string; tabId: string }
  | { type: "merge"; fromTabId: string; intoTabId: string }
  | { type: "swapPanes"; a: string; b: string }
  | { type: "setLayout"; tabId: string; layout: Layout }
  | { type: "toggleBroadcast"; tabId: string }
  | { type: "resize"; tabId: string; colSizes: number[]; rowSizes: number[] }
  | { type: "nextPane" }
  | { type: "prevPane" }
  | {
      type: "openTemplate";
      tabId: string;
      name: string;
      layout: Layout;
      colSizes: number[];
      rowSizes: number[];
      panes: Pane[];
    };

export const initialTabs: TabsState = { tabs: [{ kind: "home", id: HOME_ID }], activeId: HOME_ID };

export function isPicker(pane: AnyPane): pane is PickerPane {
  return "picker" in pane;
}

function newPane(id: string, { target, title }: Target, startup?: Startup): Pane {
  return { id, target, title, status: { state: "connecting" }, activity: "none", ...(startup ? { startup } : {}) };
}

function sessionTab(id: string, panes: AnyPane[], extra: Partial<SessionTab> = {}): SessionTab {
  return {
    kind: "session",
    id,
    panes,
    focusedId: panes[0].id,
    layout: "split",
    broadcast: false,
    colSizes: [],
    rowSizes: [],
    ...extra,
  };
}

function tabOfPane(state: TabsState, paneId: string): SessionTab | undefined {
  return state.tabs.find((t): t is SessionTab => t.kind === "session" && t.panes.some((p) => p.id === paneId));
}

function mapTab(state: TabsState, id: string, change: (tab: SessionTab) => SessionTab): TabsState {
  return { ...state, tabs: state.tabs.map((t) => (t.id === id && t.kind === "session" ? change(t) : t)) };
}

function mapPane(state: TabsState, paneId: string, change: (pane: Pane) => Pane): TabsState {
  const tab = tabOfPane(state, paneId);
  if (!tab) return state;
  return mapTab(state, tab.id, (t) => ({
    ...t,
    panes: t.panes.map((p) => (p.id === paneId && !isPicker(p) ? change(p) : p)),
  }));
}

/** The pane the user is looking at has been seen: its output and input dots go. */
function markSeen(state: TabsState): TabsState {
  const tab = state.tabs.find((t) => t.id === state.activeId);
  if (tab?.kind !== "session") return state;
  return mapPane(state, tab.focusedId, (p) =>
    p.activity === "output" || p.activity === "input" ? { ...p, activity: "none" } : p,
  );
}

function step(state: TabsState, by: number): TabsState {
  const index = state.tabs.findIndex((t) => t.id === state.activeId);
  const next = (index + by + state.tabs.length) % state.tabs.length;
  return markSeen({ ...state, activeId: state.tabs[next].id });
}

function stepPane(state: TabsState, by: number): TabsState {
  const tab = state.tabs.find((t) => t.id === state.activeId);
  if (tab?.kind !== "session") return state;
  const index = tab.panes.findIndex((p) => p.id === tab.focusedId);
  const next = tab.panes[(index + by + tab.panes.length) % tab.panes.length];
  return markSeen(mapTab(state, tab.id, (t) => ({ ...t, focusedId: next.id })));
}

function closeTab(state: TabsState, id: string): TabsState {
  const index = state.tabs.findIndex((t) => t.id === id);
  if (index < 0 || state.tabs[index].kind === "home") return state;
  const tabs = state.tabs.filter((t) => t.id !== id);
  if (state.activeId !== id) return { tabs, activeId: state.activeId };
  // Prefer the tab to the right, then the one to the left (which is at least home).
  const neighbour = tabs[index] ?? tabs[index - 1];
  return markSeen({ tabs, activeId: neighbour.id });
}

/** The tab without pane `paneId`; its sizes reset, the focus moves to a neighbour. */
function withoutPane(tab: SessionTab, paneId: string): SessionTab {
  const index = tab.panes.findIndex((p) => p.id === paneId);
  const panes = tab.panes.filter((p) => p.id !== paneId);
  const focusedId = tab.focusedId === paneId ? (panes[index] ?? panes[index - 1]).id : tab.focusedId;
  return { ...tab, panes, focusedId, colSizes: [], rowSizes: [] };
}

export function tabsReducer(state: TabsState, action: TabsAction): TabsState {
  switch (action.type) {
    case "open":
      return {
        tabs: [...state.tabs, sessionTab(action.tab.id, [newPane(action.tab.id, action.tab, action.tab.startup)])],
        activeId: action.tab.id,
      };
    case "openNew":
      return { tabs: [...state.tabs, { kind: "new", id: action.id }], activeId: action.id };
    case "pick":
      return {
        tabs: state.tabs.map((t) =>
          t.id === action.id && t.kind === "new" ? sessionTab(t.id, [newPane(t.id, action)]) : t,
        ),
        activeId: action.id,
      };
    case "pickPane": {
      const tab = tabOfPane(state, action.paneId);
      if (!tab) return state;
      return mapTab(state, tab.id, (t) => ({
        ...t,
        panes: t.panes.map((p) => (p.id === action.paneId && isPicker(p) ? newPane(p.id, action) : p)),
      }));
    }
    case "attach":
      return mapPane(state, action.paneId, (p) => ({ ...p, sessionId: action.sessionId }));
    case "status":
      return mapPane(state, action.paneId, (p) => ({
        ...p,
        status: action.status,
        activity:
          action.status.state === "ended" && action.status.reason === "error" ? "error" : p.activity,
      }));
    case "activity": {
      const tab = tabOfPane(state, action.paneId);
      if (!tab || (tab.id === state.activeId && tab.focusedId === action.paneId)) return state;
      const pane = tab.panes.find((p) => p.id === action.paneId);
      // Busy background panes report often: only a rise changes anything (and renders).
      if (!pane || isPicker(pane) || URGENCY[action.activity] <= URGENCY[pane.activity]) return state;
      return mapPane(state, action.paneId, (p) => ({ ...p, activity: action.activity }));
    }
    case "cwd": {
      const pane = tabOfPane(state, action.paneId)?.panes.find((p) => p.id === action.paneId);
      if (!pane || isPicker(pane) || pane.cwd === action.cwd) return state;
      return mapPane(state, action.paneId, (p) => ({ ...p, cwd: action.cwd }));
    }
    case "activate":
      return state.tabs.some((t) => t.id === action.id) ? markSeen({ ...state, activeId: action.id }) : state;
    case "next":
      return step(state, 1);
    case "prev":
      return step(state, -1);
    case "close":
      return closeTab(state, action.id);
    case "split":
      return mapTab(state, action.tabId, (t) =>
        t.panes.length >= MAX_PANES
          ? t
          : {
              ...t,
              panes: [...t.panes, { id: action.paneId, picker: true }],
              focusedId: action.paneId,
              colSizes: [],
              rowSizes: [],
            },
      );
    case "focusPane": {
      const tab = tabOfPane(state, action.paneId);
      if (!tab) return state;
      return markSeen({
        ...mapTab(state, tab.id, (t) => ({ ...t, focusedId: action.paneId })),
        activeId: tab.id,
      });
    }
    case "closePane": {
      const tab = tabOfPane(state, action.paneId);
      if (!tab) return state;
      if (tab.panes.length === 1) return closeTab(state, tab.id);
      return mapTab(state, tab.id, (t) => withoutPane(t, action.paneId));
    }
    case "detachPane": {
      const tab = tabOfPane(state, action.paneId);
      if (!tab || tab.panes.length === 1 || state.tabs.some((t) => t.id === action.tabId)) return state;
      const moved = tab.panes.find((p) => p.id === action.paneId) as AnyPane;
      const tabs = state.tabs.flatMap((t) =>
        t.id === tab.id ? [withoutPane(tab, action.paneId), sessionTab(action.tabId, [moved])] : [t],
      );
      return { tabs, activeId: action.tabId };
    }
    case "merge": {
      const from = state.tabs.find((t) => t.id === action.fromTabId);
      const into = state.tabs.find((t) => t.id === action.intoTabId);
      if (from?.kind !== "session" || into?.kind !== "session" || from.id === into.id) return state;
      const room = MAX_PANES - into.panes.length;
      if (room <= 0) return state;
      const moved = from.panes.slice(0, room);
      const rest = from.panes.slice(room);
      const tabs = state.tabs.flatMap((t): Tab[] => {
        if (t.id === into.id) return [{ ...into, panes: [...into.panes, ...moved], colSizes: [], rowSizes: [] }];
        if (t.id !== from.id) return [t];
        if (rest.length === 0) return [];
        const focusedId = rest.some((p) => p.id === from.focusedId) ? from.focusedId : rest[0].id;
        return [{ ...from, panes: rest, focusedId, colSizes: [], rowSizes: [] }];
      });
      return markSeen({ tabs, activeId: into.id });
    }
    case "swapPanes": {
      const tab = tabOfPane(state, action.a);
      if (!tab || !tab.panes.some((p) => p.id === action.b) || action.a === action.b) return state;
      const a = tab.panes.find((p) => p.id === action.a) as AnyPane;
      const b = tab.panes.find((p) => p.id === action.b) as AnyPane;
      return mapTab(state, tab.id, (t) => ({
        ...t,
        panes: t.panes.map((p) => (p.id === a.id ? b : p.id === b.id ? a : p)),
      }));
    }
    case "setLayout":
      return mapTab(state, action.tabId, (t) => ({ ...t, layout: action.layout }));
    case "toggleBroadcast":
      return mapTab(state, action.tabId, (t) => ({ ...t, broadcast: !t.broadcast }));
    case "resize":
      return mapTab(state, action.tabId, (t) => ({ ...t, colSizes: action.colSizes, rowSizes: action.rowSizes }));
    case "nextPane":
      return stepPane(state, 1);
    case "prevPane":
      return stepPane(state, -1);
    case "openTemplate": {
      if (action.panes.length === 0 || state.tabs.some((t) => t.id === action.tabId)) return state;
      const tab = sessionTab(action.tabId, action.panes.slice(0, MAX_PANES), {
        name: action.name,
        layout: action.layout,
        colSizes: action.colSizes,
        rowSizes: action.rowSizes,
      });
      return { tabs: [...state.tabs, tab], activeId: tab.id };
    }
  }
}

/** The tab's name, else its focused pane's title; `null` while that pane is a picker. */
export function tabTitle(tab: SessionTab): string | null {
  if (tab.name) return tab.name;
  const focused = tab.panes.find((p) => p.id === tab.focusedId);
  return focused && !isPicker(focused) ? focused.title : null;
}

/** Sessions a broadcast reaches: this tab's panes that have one. */
export function broadcastTargets(tab: SessionTab): string[] {
  return tab.panes.flatMap((p) => (!isPicker(p) && p.sessionId ? [p.sessionId] : []));
}

/** The most urgent dot of the tab's panes. */
export function tabActivity(tab: SessionTab): Activity {
  return tab.panes.reduce<Activity>(
    (most, p) => (!isPicker(p) && URGENCY[p.activity] > URGENCY[most] ? p.activity : most),
    "none",
  );
}

/** How a tab is titled: the host's name, the shell's label or "port (baud)". */
export function targetTitle(target: SessionTarget, hostName = ""): string {
  switch (target.kind) {
    case "host":
      return hostName;
    case "local":
      return target.shell.label;
    case "serial":
      return `${target.settings.port} (${target.settings.baud})`;
  }
}
