// All panes of all session tabs, in one container. A pane is only placed (CSS grid) or put off
// screen, never moved to another parent: its terminal and connection stay whatever the layout.
import { Maximize2, SquareArrowUpRight, X } from "lucide-react";
import { useRef, type CSSProperties, type Dispatch, type ReactNode } from "react";
import type { SessionStatus } from "../api";
import { useT } from "../i18n/i18n";
import { cellOf, colDividerRows, dragSizes, gridShape, normalizeSizes } from "../state/grid";
import { isPicker, type Activity, type AnyPane, type Pane, type SessionTab, type TabsAction, type TabsState } from "../state/tabs";
import { SessionStatusLine } from "./SessionStatusLine";
import { TerminalView } from "./TerminalView";

const PANE_TYPE = "application/x-sshcraft-pane";
const SIDEBAR_WIDTH = "220px";

/** Class of a pane's dot: its activity first, else how its connection is. */
export function dotClass(pane: Pane): string {
  if (pane.activity === "error") return "dot bad";
  if (pane.activity === "input") return "dot input";
  if (pane.activity === "output") return "dot output";
  switch (pane.status.state) {
    case "connected":
      return "dot ok";
    case "connecting":
    case "reconnecting":
      return "dot busy";
    case "ended":
      return pane.status.reason === "exited" ? "dot" : "dot bad";
  }
}

type Props = {
  state: TabsState;
  dispatch: Dispatch<TabsAction>;
  /** What a picker pane shows (the new-tab page for that pane). */
  renderPicker: (paneId: string) => ReactNode;
  onInput: (paneId: string, data: string, report: boolean) => void;
  onStatus: (paneId: string, status: SessionStatus) => void;
};

/** Gives the keyboard back to a pane's terminal (after a click on its title or status line). */
function focusTerminal(section: Element | null) {
  const input = section?.querySelector<HTMLElement>(".terminal-host textarea");
  input?.focus();
}

/** Clicks on these keep their own focus. */
function isControl(target: EventTarget): boolean {
  return target instanceof Element && target.closest("button, input, select, textarea, a, .terminal-host") !== null;
}

function activityLabel(activity: Activity): string | null {
  return activity === "none" ? null : `workspace.activity.${activity}`;
}

export function WorkspaceView({ state, dispatch, renderPicker, onInput, onStatus }: Props) {
  const t = useT();
  const container = useRef<HTMLDivElement>(null);
  const active = state.tabs.find((tab): tab is SessionTab => tab.id === state.activeId && tab.kind === "session");
  const many = (active?.panes.length ?? 0) > 1;
  const focusLayout = active?.layout === "focus" && many;
  const shape = gridShape(active?.panes.length ?? 1);
  const cols = normalizeSizes(active?.colSizes ?? [], shape.cols);
  const rows = normalizeSizes(active?.rowSizes ?? [], shape.rows);

  const gridStyle: CSSProperties = focusLayout
    ? { gridTemplateColumns: `${SIDEBAR_WIDTH} 1fr`, gridTemplateRows: "1fr" }
    : {
        gridTemplateColumns: cols.map((c) => `${c}fr`).join(" "),
        gridTemplateRows: rows.map((r) => `${r}fr`).join(" "),
      };

  function placement(tab: SessionTab, pane: AnyPane): { style: CSSProperties; onScreen: boolean } {
    if (tab !== active) return { style: {}, onScreen: false };
    if (focusLayout) {
      return pane.id === tab.focusedId
        ? { style: { gridColumn: "2", gridRow: "1" }, onScreen: true }
        : { style: {}, onScreen: false };
    }
    const index = tab.panes.indexOf(pane);
    const cell = cellOf(index, tab.panes.length);
    return {
      style: { gridRow: `${cell.row + 1}`, gridColumn: `${cell.col + 1} / span ${cell.colSpan}` },
      onScreen: true,
    };
  }

  /** Drags divider `index` of the columns or rows; sizes are taken from where the drag began. */
  function startDrag(event: React.PointerEvent, axis: "col" | "row", index: number) {
    const box = container.current?.getBoundingClientRect();
    if (!active || !box) return;
    event.preventDefault();
    const start = axis === "col" ? event.clientX : event.clientY;
    const length = axis === "col" ? box.width : box.height;
    const base = axis === "col" ? cols : rows;
    const tabId = active.id;
    // At most one resize per frame: every resize reaches the servers of the affected panes.
    let frame: number | null = null;
    let latest = start;
    const apply = () => {
      frame = null;
      const sizes = dragSizes(base, index, (latest - start) / length);
      dispatch({
        type: "resize",
        tabId,
        colSizes: axis === "col" ? sizes : cols,
        rowSizes: axis === "row" ? sizes : rows,
      });
    };
    const move = (e: PointerEvent) => {
      latest = axis === "col" ? e.clientX : e.clientY;
      frame ??= requestAnimationFrame(apply);
    };
    const up = () => {
      if (frame !== null) {
        cancelAnimationFrame(frame);
        apply();
      }
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function dividers(axis: "col" | "row", sizes: number[]) {
    const total = sizes.reduce((a, b) => a + b, 0);
    const rowTotal = rows.reduce((a, b) => a + b, 0);
    const count = active?.panes.length ?? 1;
    let sum = 0;
    return sizes.slice(0, -1).map((size, index) => {
      sum += size;
      const at = `${(sum / total) * 100}%`;
      // A column divider stops above a last pane that spans it.
      const covered = axis === "col" ? colDividerRows(index, count) : rows.length;
      const coveredShare = rows.slice(0, covered).reduce((a, b) => a + b, 0) / rowTotal;
      const bottom = `${(1 - coveredShare) * 100}%`;
      return (
        <div
          key={`${axis}-${index}`}
          className={`divider ${axis}`}
          role="separator"
          aria-orientation={axis === "col" ? "vertical" : "horizontal"}
          aria-label={t("workspace.resize")}
          style={axis === "col" ? { left: at, bottom } : { top: at }}
          onPointerDown={(e) => startDrag(e, axis, index)}
        />
      );
    });
  }

  const sessionTabs = state.tabs.filter((tab): tab is SessionTab => tab.kind === "session");
  const classes = ["workspace", active ? "" : "hidden", active?.broadcast ? "broadcast" : ""].filter(Boolean);

  return (
    <div className={classes.join(" ")} ref={container} style={gridStyle}>
      {focusLayout && active && (
        <ul className="pane-list" aria-label={t("workspace.panes")}>
          {active.panes.map((pane) => (
            <li key={pane.id}>
              <button
                className={pane.id === active.focusedId ? "pane-entry active" : "pane-entry"}
                onClick={() => dispatch({ type: "focusPane", paneId: pane.id })}
              >
                {!isPicker(pane) && <span className={dotClass(pane)} aria-hidden="true" />}
                <span>{isPicker(pane) ? t("tabs.newTitle") : pane.title}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {sessionTabs.flatMap((tab) =>
        tab.panes.map((pane) => {
          const { style, onScreen } = placement(tab, pane);
          const focused = tab === active && pane.id === tab.focusedId;
          const label = isPicker(pane) ? null : activityLabel(pane.activity);
          return (
            <section
              key={pane.id}
              data-pane={pane.id}
              className={["ws-pane", onScreen ? "" : "offscreen", focused && many ? "focused" : ""]
                .filter(Boolean)
                .join(" ")}
              style={style}
              onMouseDown={() => {
                if (!focused && tab === active) dispatch({ type: "focusPane", paneId: pane.id });
              }}
              onClick={(e) => {
                // A click on the title or status line must not leave the keyboard nowhere.
                if (!isPicker(pane) && !isControl(e.target)) focusTerminal(e.currentTarget);
              }}
            >
              {tab.panes.length > 1 && (
                <header
                  className="pane-header"
                  draggable
                  onDragStart={(e) => e.dataTransfer.setData(PANE_TYPE, pane.id)}
                  onDragOver={(e) => {
                    if (e.dataTransfer.types.includes(PANE_TYPE)) e.preventDefault();
                  }}
                  onDrop={(e) => {
                    const other = e.dataTransfer.getData(PANE_TYPE);
                    if (other) dispatch({ type: "swapPanes", a: other, b: pane.id });
                  }}
                  onDragEnd={(e) => focusTerminal(e.currentTarget.closest(".ws-pane"))}
                >
                  {!isPicker(pane) && (
                    <span className={dotClass(pane)} role="img" aria-label={label ? t(label) : undefined} />
                  )}
                  <span className="pane-title">{isPicker(pane) ? t("tabs.newTitle") : pane.title}</span>
                  <button
                    className="icon"
                    aria-label={t("workspace.focus")}
                    title={t("workspace.focus")}
                    onClick={() => {
                      dispatch({ type: "focusPane", paneId: pane.id });
                      dispatch({ type: "setLayout", tabId: tab.id, layout: "focus" });
                    }}
                  >
                    <Maximize2 size={13} />
                  </button>
                  <button
                    className="icon"
                    aria-label={t("workspace.detach")}
                    title={t("workspace.detach")}
                    onClick={() => dispatch({ type: "detachPane", paneId: pane.id, tabId: crypto.randomUUID() })}
                  >
                    <SquareArrowUpRight size={13} />
                  </button>
                  <button
                    className="icon"
                    aria-label={t("workspace.closePane")}
                    title={t("workspace.closePane")}
                    onClick={() => dispatch({ type: "closePane", paneId: pane.id })}
                  >
                    <X size={13} />
                  </button>
                </header>
              )}
              {isPicker(pane) ? (
                <div className="pane-picker" data-testid="pane-picker">
                  {renderPicker(pane.id)}
                </div>
              ) : (
                <>
                  <TerminalView
                    pane={pane}
                    visible={onScreen}
                    focused={focused}
                    onAttach={(sessionId) => dispatch({ type: "attach", paneId: pane.id, sessionId })}
                    onStatus={(status) => onStatus(pane.id, status)}
                    onInput={(data, report) => onInput(pane.id, data, report)}
                    onActivity={(activity) => dispatch({ type: "activity", paneId: pane.id, activity })}
                    onCwd={(cwd) => dispatch({ type: "cwd", paneId: pane.id, cwd })}
                    onFocus={() => {
                      if (!focused) dispatch({ type: "focusPane", paneId: pane.id });
                    }}
                  />
                  <SessionStatusLine status={pane.status} />
                </>
              )}
            </section>
          );
        }),
      )}
      {active && many && !focusLayout && dividers("col", cols)}
      {active && many && !focusLayout && dividers("row", rows)}
    </div>
  );
}
