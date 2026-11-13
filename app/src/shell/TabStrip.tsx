import { Columns2, PanelLeft, Plus, Radio, Save, X } from "lucide-react";
import type { Dispatch } from "react";
import logo from "../assets/logo.svg";
import { dotClass } from "../components/WorkspaceView";
import { useT } from "../i18n/i18n";
import {
  MAX_PANES,
  isPicker,
  tabActivity,
  tabTitle,
  type SessionTab,
  type Tab,
  type TabsAction,
  type TabsState,
} from "../state/tabs";

const TAB_TYPE = "application/x-sshcraft-tab";

/** The tab's dot: the most urgent pane, else the connection of the focused pane. */
function statusClass(tab: Tab): string {
  if (tab.kind !== "session") return "";
  const activity = tabActivity(tab);
  if (activity === "error") return "dot bad";
  if (activity !== "none") return `dot ${activity}`;
  const focused = tab.panes.find((p) => p.id === tab.focusedId);
  return focused && !isPicker(focused) ? dotClass(focused) : "";
}

type Props = {
  state: TabsState;
  dispatch: Dispatch<TabsAction>;
  onNewTab: () => void;
  onSplit: () => void;
  onSaveTemplate: (tab: SessionTab) => void;
};

/** The strip at the top: start page, one entry per tab, "+" for a new tab, workspace tools. */
export function TabStrip({ state, dispatch, onNewTab, onSplit, onSaveTemplate }: Props) {
  const t = useT();
  const active = state.tabs.find((tab): tab is SessionTab => tab.id === state.activeId && tab.kind === "session");
  return (
    <div className="tabstrip" role="tablist">
      {state.tabs.map((tab) => {
        const selected = tab.id === state.activeId;
        if (tab.kind === "home") {
          return (
            <button
              key={tab.id}
              role="tab"
              aria-selected={selected}
              aria-label={t("tabs.home")}
              className={selected ? "tab home active" : "tab home"}
              onClick={() => dispatch({ type: "activate", id: tab.id })}
            >
              <img src={logo} alt="" />
              <span>SSHCraft</span>
            </button>
          );
        }
        const title = (tab.kind === "session" ? tabTitle(tab) : null) ?? t("tabs.newTitle");
        const session = tab.kind === "session";
        return (
          <div key={tab.id} className={selected ? "tab-item active" : "tab-item"}>
            <button
              role="tab"
              aria-selected={selected}
              className="tab"
              onClick={() => dispatch({ type: "activate", id: tab.id })}
              draggable={session}
              onDragStart={(e) => e.dataTransfer.setData(TAB_TYPE, tab.id)}
              onDragOver={(e) => {
                if (session && e.dataTransfer.types.includes(TAB_TYPE)) e.preventDefault();
              }}
              onDrop={(e) => {
                const from = e.dataTransfer.getData(TAB_TYPE);
                if (session && from && from !== tab.id) dispatch({ type: "merge", fromTabId: from, intoTabId: tab.id });
              }}
            >
              <span className={statusClass(tab)} aria-hidden="true" />
              <span className="tab-title">{title}</span>
              {tab.kind === "session" && tab.panes.length > 1 && <span className="tab-count">{tab.panes.length}</span>}
              {tab.kind === "session" && tab.broadcast && <Radio size={12} className="tab-broadcast" aria-hidden="true" />}
            </button>
            <button
              className="icon tab-close"
              aria-label={t("tabs.close")}
              title={t("tabs.close")}
              onClick={() => dispatch({ type: "close", id: tab.id })}
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
      <button className="icon tab-new" aria-label={t("tabs.new")} title={t("tabs.new")} onClick={onNewTab}>
        <Plus size={16} />
      </button>
      {active && (
        <div className="tab-tools">
          <button
            className="icon"
            aria-label={t("workspace.split")}
            title={t("workspace.split")}
            disabled={active.panes.length >= MAX_PANES}
            onClick={onSplit}
          >
            <Columns2 size={15} />
          </button>
          {active.panes.length > 1 && (
            <button
              className={active.layout === "focus" ? "icon on" : "icon"}
              aria-label={t("workspace.focusLayout")}
              aria-pressed={active.layout === "focus"}
              title={t("workspace.focusLayout")}
              onClick={() =>
                dispatch({ type: "setLayout", tabId: active.id, layout: active.layout === "focus" ? "split" : "focus" })
              }
            >
              <PanelLeft size={15} />
            </button>
          )}
          {active.panes.length > 1 && (
            <button
              className={active.broadcast ? "icon on broadcast" : "icon"}
              aria-label={t("workspace.broadcast")}
              aria-pressed={active.broadcast}
              title={t("workspace.broadcast")}
              onClick={() => dispatch({ type: "toggleBroadcast", tabId: active.id })}
            >
              <Radio size={15} />
            </button>
          )}
          <button
            className="icon"
            aria-label={t("workspace.saveTemplate")}
            title={t("workspace.saveTemplate")}
            onClick={() => onSaveTemplate(active)}
          >
            <Save size={15} />
          </button>
        </div>
      )}
    </div>
  );
}
