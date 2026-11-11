import { expect, test } from "vitest";
import { initialTabs, tabsReducer, type TabsState } from "./tabs";

function open(state: TabsState, id: string): TabsState {
  return tabsReducer(state, { type: "open", tab: { id, hostId: `h-${id}`, title: id } });
}

test("opening a tab activates it", () => {
  const s = open(open(initialTabs, "a"), "b");
  expect(s.activeId).toBe("b");
  expect(s.tabs.map((t) => t.id)).toEqual(["a", "b"]);
  expect(s.tabs[1].status).toEqual({ state: "connecting" });
});

test("closing the active tab activates its neighbour", () => {
  let s = open(open(open(initialTabs, "a"), "b"), "c");
  s = tabsReducer(s, { type: "activate", id: "b" });
  s = tabsReducer(s, { type: "close", id: "b" });
  expect(s.activeId).toBe("c");
  s = tabsReducer(s, { type: "close", id: "c" });
  expect(s.activeId).toBe("a");
  s = tabsReducer(s, { type: "close", id: "a" });
  expect(s.activeId).toBeNull();
});

test("closing an inactive tab keeps the active one", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = tabsReducer(s, { type: "close", id: "a" });
  expect(s.activeId).toBe("b");
});

test("status updates only touch the matching tab", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = tabsReducer(s, { type: "status", id: "a", status: { state: "connected" } });
  expect(s.tabs[0].status).toEqual({ state: "connected" });
  expect(s.tabs[1].status).toEqual({ state: "connecting" });
});

test("attach stores the session id", () => {
  let s = open(initialTabs, "a");
  s = tabsReducer(s, { type: "attach", id: "a", sessionId: "s1" });
  expect(s.tabs[0].sessionId).toBe("s1");
});
