import { Plus, X } from "lucide-react";
import type { Dispatch } from "react";
import logo from "../assets/logo.svg";
import { useT } from "../i18n/i18n";
import type { Tab, TabsAction, TabsState } from "../state/tabs";

function statusClass(tab: Tab): string {
  if (tab.kind !== "session") return "";
  switch (tab.status.state) {
    case "connected":
      return "dot ok";
    case "connecting":
    case "reconnecting":
      return "dot busy";
    case "ended":
      return tab.status.reason === "exited" ? "dot" : "dot bad";
  }
}

type Props = { state: TabsState; dispatch: Dispatch<TabsAction>; onNewTab: () => void };

/** The strip at the top: start page, one entry per tab, and "+" for a new tab. */
export function TabStrip({ state, dispatch, onNewTab }: Props) {
  const t = useT();
  return (
    <div className="tabstrip" role="tablist">
      {state.tabs.map((tab) => {
        const active = tab.id === state.activeId;
        if (tab.kind === "home") {
          return (
            <button
              key={tab.id}
              role="tab"
              aria-selected={active}
              aria-label={t("tabs.home")}
              className={active ? "tab home active" : "tab home"}
              onClick={() => dispatch({ type: "activate", id: tab.id })}
            >
              <img src={logo} alt="" />
              <span>SSHCraft</span>
            </button>
          );
        }
        const title = tab.kind === "session" ? tab.title : t("tabs.newTitle");
        return (
          <div key={tab.id} className={active ? "tab-item active" : "tab-item"}>
            <button role="tab" aria-selected={active} className="tab" onClick={() => dispatch({ type: "activate", id: tab.id })}>
              <span className={statusClass(tab)} aria-hidden="true" />
              <span className="tab-title">{title}</span>
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
    </div>
  );
}
