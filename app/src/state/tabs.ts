// Tabs of the window: the start page (always first), new-tab pages and terminal sessions.
import type { SessionStatus, SessionTarget } from "../api";

export const HOME_ID = "home";

export type HomeTab = { kind: "home"; id: typeof HOME_ID };
export type NewTab = { kind: "new"; id: string };
export type SessionTab = {
  kind: "session";
  id: string;
  target: SessionTarget;
  title: string;
  sessionId?: string;
  status: SessionStatus;
};
export type Tab = HomeTab | NewTab | SessionTab;

export type TabsState = { tabs: Tab[]; activeId: string };

export type TabsAction =
  | { type: "open"; tab: { id: string; target: SessionTarget; title: string } }
  | { type: "openNew"; id: string }
  | { type: "pick"; id: string; target: SessionTarget; title: string }
  | { type: "attach"; id: string; sessionId: string }
  | { type: "status"; id: string; status: SessionStatus }
  | { type: "close"; id: string }
  | { type: "activate"; id: string }
  | { type: "next" }
  | { type: "prev" };

export const initialTabs: TabsState = { tabs: [{ kind: "home", id: HOME_ID }], activeId: HOME_ID };

function updateSession(state: TabsState, id: string, change: (tab: SessionTab) => SessionTab): TabsState {
  return { ...state, tabs: state.tabs.map((t) => (t.id === id && t.kind === "session" ? change(t) : t)) };
}

function step(state: TabsState, by: number): TabsState {
  const index = state.tabs.findIndex((t) => t.id === state.activeId);
  const next = (index + by + state.tabs.length) % state.tabs.length;
  return { ...state, activeId: state.tabs[next].id };
}

export function tabsReducer(state: TabsState, action: TabsAction): TabsState {
  switch (action.type) {
    case "open":
      return {
        tabs: [...state.tabs, { kind: "session", ...action.tab, status: { state: "connecting" } }],
        activeId: action.tab.id,
      };
    case "openNew":
      return { tabs: [...state.tabs, { kind: "new", id: action.id }], activeId: action.id };
    case "pick":
      return {
        tabs: state.tabs.map((t) =>
          t.id === action.id && t.kind === "new"
            ? { kind: "session", id: t.id, target: action.target, title: action.title, status: { state: "connecting" } }
            : t,
        ),
        activeId: action.id,
      };
    case "attach":
      return updateSession(state, action.id, (t) => ({ ...t, sessionId: action.sessionId }));
    case "status":
      return updateSession(state, action.id, (t) => ({ ...t, status: action.status }));
    case "activate":
      return state.tabs.some((t) => t.id === action.id) ? { ...state, activeId: action.id } : state;
    case "next":
      return step(state, 1);
    case "prev":
      return step(state, -1);
    case "close": {
      const index = state.tabs.findIndex((t) => t.id === action.id);
      if (index < 0 || state.tabs[index].kind === "home") return state;
      const tabs = state.tabs.filter((t) => t.id !== action.id);
      if (state.activeId !== action.id) return { tabs, activeId: state.activeId };
      // Prefer the tab to the right, then the one to the left (which is at least home).
      const neighbour = tabs[index] ?? tabs[index - 1];
      return { tabs, activeId: neighbour.id };
    }
  }
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
