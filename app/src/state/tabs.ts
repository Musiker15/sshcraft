// Terminal tabs: which are open, which is active, and each one's session status.
import type { SessionStatus } from "../api";

export type Tab = {
  id: string;
  hostId: string;
  title: string;
  sessionId?: string;
  status: SessionStatus;
};

export type TabsState = { tabs: Tab[]; activeId: string | null };

export type TabsAction =
  | { type: "open"; tab: Omit<Tab, "status" | "sessionId"> }
  | { type: "attach"; id: string; sessionId: string }
  | { type: "status"; id: string; status: SessionStatus }
  | { type: "close"; id: string }
  | { type: "activate"; id: string };

export const initialTabs: TabsState = { tabs: [], activeId: null };

export function tabsReducer(state: TabsState, action: TabsAction): TabsState {
  switch (action.type) {
    case "open":
      return {
        tabs: [...state.tabs, { ...action.tab, status: { state: "connecting" } }],
        activeId: action.tab.id,
      };
    case "attach":
      return {
        ...state,
        tabs: state.tabs.map((t) => (t.id === action.id ? { ...t, sessionId: action.sessionId } : t)),
      };
    case "status":
      return {
        ...state,
        tabs: state.tabs.map((t) => (t.id === action.id ? { ...t, status: action.status } : t)),
      };
    case "activate":
      return state.tabs.some((t) => t.id === action.id) ? { ...state, activeId: action.id } : state;
    case "close": {
      const index = state.tabs.findIndex((t) => t.id === action.id);
      if (index < 0) return state;
      const tabs = state.tabs.filter((t) => t.id !== action.id);
      if (state.activeId !== action.id) return { tabs, activeId: state.activeId };
      const neighbour = tabs[index] ?? tabs[index - 1] ?? null;
      return { tabs, activeId: neighbour ? neighbour.id : null };
    }
  }
}
