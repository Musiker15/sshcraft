import { expect, test } from "vitest";
import { HOME_ID, initialTabs, tabsReducer, type SessionTab, type TabsState } from "./tabs";

function open(state: TabsState, id: string): TabsState {
  return tabsReducer(state, { type: "open", tab: { id, hostId: `h-${id}`, title: id } });
}

const ids = (s: TabsState) => s.tabs.map((t) => t.id);
const session = (s: TabsState, id: string) => s.tabs.find((t) => t.id === id) as SessionTab;

test("the start page is there from the beginning and active", () => {
  expect(ids(initialTabs)).toEqual([HOME_ID]);
  expect(initialTabs.activeId).toBe(HOME_ID);
});

test("opening a tab activates it", () => {
  const s = open(open(initialTabs, "a"), "b");
  expect(s.activeId).toBe("b");
  expect(ids(s)).toEqual([HOME_ID, "a", "b"]);
  expect(session(s, "b").status).toEqual({ state: "connecting" });
});

test("the start page cannot be closed", () => {
  const s = tabsReducer(open(initialTabs, "a"), { type: "close", id: HOME_ID });
  expect(ids(s)).toEqual([HOME_ID, "a"]);
});

test("a new-tab page becomes a session in place", () => {
  let s = open(initialTabs, "a");
  s = tabsReducer(s, { type: "openNew", id: "n" });
  s = open(s, "b");
  expect(s.tabs.find((t) => t.id === "n")?.kind).toBe("new");
  s = tabsReducer(s, { type: "pick", id: "n", hostId: "h-x", title: "x" });
  expect(ids(s)).toEqual([HOME_ID, "a", "n", "b"]);
  expect(session(s, "n")).toMatchObject({ kind: "session", hostId: "h-x", title: "x", status: { state: "connecting" } });
  expect(s.activeId).toBe("n");
});

test("closing the active tab activates its neighbour, the last one returns home", () => {
  let s = open(open(open(initialTabs, "a"), "b"), "c");
  s = tabsReducer(s, { type: "activate", id: "b" });
  s = tabsReducer(s, { type: "close", id: "b" });
  expect(s.activeId).toBe("c");
  s = tabsReducer(s, { type: "close", id: "c" });
  expect(s.activeId).toBe("a");
  s = tabsReducer(s, { type: "close", id: "a" });
  expect(s.activeId).toBe(HOME_ID);
});

test("closing an inactive tab keeps the active one", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = tabsReducer(s, { type: "close", id: "a" });
  expect(s.activeId).toBe("b");
});

test("next and previous go round over all tabs", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = tabsReducer(s, { type: "next" });
  expect(s.activeId).toBe(HOME_ID);
  s = tabsReducer(s, { type: "next" });
  expect(s.activeId).toBe("a");
  s = tabsReducer(s, { type: "prev" });
  expect(s.activeId).toBe(HOME_ID);
  s = tabsReducer(s, { type: "prev" });
  expect(s.activeId).toBe("b");
});

test("status updates only touch the matching tab", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = tabsReducer(s, { type: "status", id: "a", status: { state: "connected" } });
  expect(session(s, "a").status).toEqual({ state: "connected" });
  expect(session(s, "b").status).toEqual({ state: "connecting" });
});

test("attach stores the session id", () => {
  let s = open(initialTabs, "a");
  s = tabsReducer(s, { type: "attach", id: "a", sessionId: "s1" });
  expect(session(s, "a").sessionId).toBe("s1");
});
