import { expect, test } from "vitest";
import {
  HOME_ID,
  MAX_PANES,
  broadcastTargets,
  initialTabs,
  tabActivity,
  tabTitle,
  tabsReducer,
  targetTitle,
  type Pane,
  type SessionTab,
  type TabsAction,
  type TabsState,
} from "./tabs";

function open(state: TabsState, id: string): TabsState {
  return tabsReducer(state, { type: "open", tab: { id, target: { kind: "host", host_id: `h-${id}` }, title: id } });
}
const run = (s: TabsState, ...actions: TabsAction[]) => actions.reduce(tabsReducer, s);
const ids = (s: TabsState) => s.tabs.map((t) => t.id);
const session = (s: TabsState, id: string) => s.tabs.find((t) => t.id === id) as SessionTab;
const pane = (s: TabsState, id: string) =>
  s.tabs.flatMap((t) => (t.kind === "session" ? t.panes : [])).find((p) => p.id === id) as Pane;
const paneIds = (s: TabsState, tabId: string) => session(s, tabId).panes.map((p) => p.id);
const pick = (paneId: string): TabsAction => ({
  type: "pickPane",
  paneId,
  target: { kind: "host", host_id: `h-${paneId}` },
  title: paneId,
});
const failed = { state: "ended", reason: "error", code: null, detail: null } as const;

test("the start page is there from the beginning and active", () => {
  expect(ids(initialTabs)).toEqual([HOME_ID]);
  expect(initialTabs.activeId).toBe(HOME_ID);
});

test("opening a tab activates it with one pane", () => {
  const s = open(open(initialTabs, "a"), "b");
  expect(s.activeId).toBe("b");
  expect(ids(s)).toEqual([HOME_ID, "a", "b"]);
  expect(paneIds(s, "b")).toEqual(["b"]);
  expect(pane(s, "b").status).toEqual({ state: "connecting" });
  expect(session(s, "b")).toMatchObject({ focusedId: "b", layout: "split", broadcast: false });
});

test("the start page cannot be closed", () => {
  const s = tabsReducer(open(initialTabs, "a"), { type: "close", id: HOME_ID });
  expect(ids(s)).toEqual([HOME_ID, "a"]);
});

test("a new-tab page becomes a session in place", () => {
  let s = open(initialTabs, "a");
  s = run(s, { type: "openNew", id: "n" });
  s = open(s, "b");
  expect(s.tabs.find((t) => t.id === "n")?.kind).toBe("new");
  s = run(s, { type: "pick", id: "n", target: { kind: "host", host_id: "h-x" }, title: "x" });
  expect(ids(s)).toEqual([HOME_ID, "a", "n", "b"]);
  expect(pane(s, "n")).toMatchObject({
    target: { kind: "host", host_id: "h-x" },
    title: "x",
    status: { state: "connecting" },
  });
  expect(s.activeId).toBe("n");
});

test("closing the active tab activates its neighbour, the last one returns home", () => {
  let s = open(open(open(initialTabs, "a"), "b"), "c");
  s = run(s, { type: "activate", id: "b" }, { type: "close", id: "b" });
  expect(s.activeId).toBe("c");
  s = run(s, { type: "close", id: "c" });
  expect(s.activeId).toBe("a");
  s = run(s, { type: "close", id: "a" });
  expect(s.activeId).toBe(HOME_ID);
});

test("closing an inactive tab keeps the active one", () => {
  const s = run(open(open(initialTabs, "a"), "b"), { type: "close", id: "a" });
  expect(s.activeId).toBe("b");
});

test("next and previous go round over all tabs", () => {
  let s = open(open(initialTabs, "a"), "b");
  s = run(s, { type: "next" });
  expect(s.activeId).toBe(HOME_ID);
  s = run(s, { type: "next" });
  expect(s.activeId).toBe("a");
  s = run(s, { type: "prev" }, { type: "prev" });
  expect(s.activeId).toBe("b");
});

test("status updates only touch the matching pane", () => {
  const s = run(open(open(initialTabs, "a"), "b"), { type: "status", paneId: "a", status: { state: "connected" } });
  expect(pane(s, "a").status).toEqual({ state: "connected" });
  expect(pane(s, "b").status).toEqual({ state: "connecting" });
});

test("attach stores the session id", () => {
  const s = run(open(initialTabs, "a"), { type: "attach", paneId: "a", sessionId: "s1" });
  expect(pane(s, "a").sessionId).toBe("s1");
});

test("a local shell tab is titled by its label, a serial one by port and speed", () => {
  const shell = { program: "pwsh.exe", args: [], label: "PowerShell" };
  let s = run(initialTabs, {
    type: "open",
    tab: { id: "l", target: { kind: "local", shell }, title: targetTitle({ kind: "local", shell }) },
  });
  expect(tabTitle(session(s, "l"))).toBe("PowerShell");
  const settings = { port: "COM3", baud: 9600, data_bits: 8, stop_bits: 1, parity: "none" as const, flow: "none" as const };
  s = run(s, {
    type: "open",
    tab: { id: "c", target: { kind: "serial", settings }, title: targetTitle({ kind: "serial", settings }) },
  });
  expect(tabTitle(session(s, "c"))).toBe("COM3 (9600)");
});

test("split adds a picker pane, focuses it and stops at 16", () => {
  let s = run(open(initialTabs, "a"), { type: "split", tabId: "a", paneId: "p1" });
  expect(paneIds(s, "a")).toEqual(["a", "p1"]);
  expect(session(s, "a").focusedId).toBe("p1");
  expect(tabTitle(session(s, "a"))).toBeNull();
  s = run(s, pick("p1"));
  expect(pane(s, "p1").title).toBe("p1");
  for (let i = 2; i <= 20; i++) s = run(s, { type: "split", tabId: "a", paneId: `p${i}` });
  expect(paneIds(s, "a")).toHaveLength(MAX_PANES);
});

test("closing the last pane closes the tab", () => {
  let s = run(open(open(initialTabs, "a"), "b"), { type: "split", tabId: "a", paneId: "p" });
  s = run(s, { type: "closePane", paneId: "p" });
  expect(paneIds(s, "a")).toEqual(["a"]);
  expect(session(s, "a").focusedId).toBe("a");
  s = run(s, { type: "closePane", paneId: "a" });
  expect(ids(s)).toEqual([HOME_ID, "b"]);
});

test("detach moves the pane to a new tab next to it, keeping its session", () => {
  let s = run(
    open(open(initialTabs, "a"), "z"),
    { type: "split", tabId: "a", paneId: "p" },
    pick("p"),
    { type: "attach", paneId: "p", sessionId: "s-p" },
    { type: "detachPane", paneId: "p", tabId: "t2" },
  );
  expect(ids(s)).toEqual([HOME_ID, "a", "t2", "z"]);
  expect(paneIds(s, "t2")).toEqual(["p"]);
  expect(pane(s, "p").sessionId).toBe("s-p");
  expect(s.activeId).toBe("t2");
  // A tab with a single pane is already on its own.
  s = run(s, { type: "detachPane", paneId: "p", tabId: "t3" });
  expect(ids(s)).toEqual([HOME_ID, "a", "t2", "z"]);
});

test("merge appends up to 16 panes and leaves the rest", () => {
  let s = open(open(initialTabs, "a"), "b");
  for (let i = 1; i < 15; i++) s = run(s, { type: "split", tabId: "a", paneId: `a${i}` });
  s = run(s, { type: "split", tabId: "b", paneId: "b1" }, { type: "split", tabId: "b", paneId: "b2" });
  s = run(s, { type: "merge", fromTabId: "b", intoTabId: "a" });
  expect(paneIds(s, "a")).toHaveLength(16);
  expect(paneIds(s, "a").slice(-1)).toEqual(["b"]);
  expect(paneIds(s, "b")).toEqual(["b1", "b2"]);
  s = run(s, { type: "closePane", paneId: "a1" }, { type: "closePane", paneId: "a2" });
  s = run(s, { type: "merge", fromTabId: "b", intoTabId: "a" });
  expect(ids(s)).not.toContain("b");
  expect(s.activeId).toBe("a");
});

test("swap exchanges two panes of a tab", () => {
  const s = run(
    open(initialTabs, "a"),
    { type: "split", tabId: "a", paneId: "p" },
    { type: "swapPanes", a: "a", b: "p" },
  );
  expect(paneIds(s, "a")).toEqual(["p", "a"]);
});

test("broadcast reaches only this tab's panes that have a session", () => {
  const s = run(
    open(open(initialTabs, "a"), "b"),
    { type: "split", tabId: "a", paneId: "p" },
    pick("p"),
    { type: "split", tabId: "a", paneId: "q" },
    // Connecting, no session yet.
    pick("q"),
    { type: "split", tabId: "a", paneId: "r" },
    { type: "attach", paneId: "a", sessionId: "s-a" },
    { type: "attach", paneId: "p", sessionId: "s-p" },
    { type: "attach", paneId: "b", sessionId: "s-b" },
  );
  expect(broadcastTargets(session(s, "a"))).toEqual(["s-a", "s-p"]);
});

test("broadcast and layout are switched per tab", () => {
  const s = run(
    open(open(initialTabs, "a"), "b"),
    { type: "toggleBroadcast", tabId: "a" },
    { type: "setLayout", tabId: "a", layout: "focus" },
  );
  expect(session(s, "a")).toMatchObject({ broadcast: true, layout: "focus" });
  expect(session(s, "b")).toMatchObject({ broadcast: false, layout: "split" });
});

test("activity on the focused pane of the active tab is ignored", () => {
  let s = run(open(initialTabs, "a"), { type: "split", tabId: "a", paneId: "p" }, pick("p"));
  s = run(s, { type: "activity", paneId: "p", activity: "output" });
  expect(pane(s, "p").activity).toBe("none");
  s = run(s, { type: "activity", paneId: "a", activity: "output" });
  expect(pane(s, "a").activity).toBe("output");
});

test("activity only rises until seen; focusing clears output and input, not error", () => {
  let s = run(open(open(initialTabs, "a"), "b"), { type: "split", tabId: "b", paneId: "q" }, pick("q"));
  s = run(s, { type: "activity", paneId: "a", activity: "input" }, { type: "activity", paneId: "a", activity: "output" });
  expect(pane(s, "a").activity).toBe("input");
  s = run(s, { type: "activate", id: "a" });
  expect(pane(s, "a").activity).toBe("none");
  s = run(s, { type: "status", paneId: "b", status: failed });
  expect(pane(s, "b").activity).toBe("error");
  s = run(s, { type: "focusPane", paneId: "b" });
  expect(s.activeId).toBe("b");
  expect(session(s, "b").focusedId).toBe("b");
  expect(pane(s, "b").activity).toBe("error");
});

test("a tab shows its most urgent pane", () => {
  let s = run(
    open(open(initialTabs, "a"), "b"),
    { type: "split", tabId: "a", paneId: "p" },
    pick("p"),
    { type: "split", tabId: "a", paneId: "q" },
    pick("q"),
  );
  s = run(s, { type: "activity", paneId: "a", activity: "output" });
  expect(tabActivity(session(s, "a"))).toBe("output");
  s = run(s, { type: "activity", paneId: "p", activity: "input" });
  expect(tabActivity(session(s, "a"))).toBe("input");
  s = run(s, { type: "status", paneId: "q", status: failed });
  expect(tabActivity(session(s, "a"))).toBe("error");
});

test("next and previous pane go round in the active tab", () => {
  let s = run(open(initialTabs, "a"), { type: "split", tabId: "a", paneId: "p" }, pick("p"));
  s = run(s, { type: "nextPane" });
  expect(session(s, "a").focusedId).toBe("a");
  s = run(s, { type: "prevPane" });
  expect(session(s, "a").focusedId).toBe("p");
});

test("a template opens as a new active tab with broadcast off", () => {
  const panes: Pane[] = ["x", "y"].map((id) => ({
    id,
    target: { kind: "host", host_id: `h-${id}` },
    title: id,
    status: { state: "connecting" },
    activity: "none",
    startup: { command: "htop" },
  }));
  const s = run(open(initialTabs, "a"), {
    type: "openTemplate",
    tabId: "w",
    name: "Web",
    layout: "focus",
    colSizes: [2, 1],
    rowSizes: [1],
    panes,
  });
  expect(s.activeId).toBe("w");
  expect(session(s, "w")).toMatchObject({
    name: "Web",
    layout: "focus",
    broadcast: false,
    focusedId: "x",
    colSizes: [2, 1],
  });
  expect(tabTitle(session(s, "w"))).toBe("Web");
});

test("resize stores the sizes and a pane count change resets them", () => {
  let s = run(open(initialTabs, "a"), { type: "split", tabId: "a", paneId: "p" });
  s = run(s, { type: "resize", tabId: "a", colSizes: [3, 1], rowSizes: [1] });
  expect(session(s, "a").colSizes).toEqual([3, 1]);
  s = run(s, { type: "split", tabId: "a", paneId: "q" });
  expect(session(s, "a").colSizes).toEqual([]);
});

test("the working directory a shell reports is kept", () => {
  const s = run(open(initialTabs, "a"), { type: "cwd", paneId: "a", cwd: "/srv" });
  expect(pane(s, "a").cwd).toBe("/srv");
});
