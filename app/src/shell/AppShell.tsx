// The window: tab strip on top, the start page (navigation plus page) or a terminal below.
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  answerHostKey,
  deleteGroup,
  deleteHost,
  helloStatus,
  listGroups,
  listHosts,
  listKeys,
  listRecent,
  listServers,
  moveItem,
  vaultStatus,
  type Group,
  type Host,
  type HostKeyPrompt,
  type ItemKind,
  type KeyInfo,
  type ServerView,
  type VaultStatus,
} from "../api";
import { GroupForm } from "../components/GroupForm";
import { HostForm } from "../components/HostForm";
import { HostKeyDialog } from "../components/HostKeyDialog";
import { SessionStatusLine } from "../components/SessionStatusLine";
import { SshConfigImport } from "../components/SshConfigImport";
import { TerminalView } from "../components/TerminalView";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { HostsPage } from "../pages/HostsPage";
import { KeysPage } from "../pages/KeysPage";
import { NewTabPage } from "../pages/NewTabPage";
import { CommandPalette } from "../palette/CommandPalette";
import type { PaletteActionId } from "../palette/rank";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { SettingsPage } from "../pages/SettingsPage";
import { HOME_ID, initialTabs, tabsReducer } from "../state/tabs";
import { ConfirmDialog } from "./ConfirmDialog";
import { Drawer } from "./Drawer";
import { ErrorMessage } from "./ErrorMessage";
import { NavSidebar, type Page } from "./NavSidebar";
import { matchShortcut, type ShortcutAction } from "./shortcuts";
import { TabStrip } from "./TabStrip";

type Editor = { kind: "host"; host?: Host } | { kind: "group"; group?: Group } | { kind: "sshconfig" } | null;
type Confirm = { message: string; confirmLabel: string; action: () => Promise<void> } | null;

export function AppShell() {
  const t = useT();
  const [groups, setGroups] = useState<Group[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [servers, setServers] = useState<ServerView[]>([]);
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [problem, setProblem] = useState<unknown>(null);
  const [editor, setEditor] = useState<Editor>(null);
  const [confirming, setConfirming] = useState<Confirm>(null);
  const [prompts, setPrompts] = useState<HostKeyPrompt[]>([]);
  const [tabs, dispatch] = useReducer(tabsReducer, initialTabs);
  const [page, setPage] = useState<Page>("hosts");
  const [helloMissing, setHelloMissing] = useState(false);
  const [helloDismissed, setHelloDismissed] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);

  const reloadRecent = useCallback(() => {
    listRecent(20).then(setRecent, () => {});
  }, []);

  const reload = useCallback(async () => {
    try {
      const [g, h, k, s] = await Promise.all([listGroups(), listHosts(), listKeys(), listServers()]);
      setGroups(g);
      setHosts(h);
      setKeys(k);
      setServers(s ?? []);
      setProblem(null);
    } catch (e) {
      setProblem(e);
    }
  }, []);

  useEffect(() => {
    void reload();
    reloadRecent();
    void vaultStatus().then((s) => setVault(s.ok ? null : s));
    // Hello is only checked when a stored key is first used; the core reports the outcome.
    void helloStatus().then((s) => setHelloMissing(s.missing));
    const listeners = [
      listen<HostKeyPrompt>("host-key-prompt", (event) => setPrompts((queue) => [...queue, event.payload])),
      // The tab that asked was closed: its question must not stay on screen.
      listen<{ prompt_id: number }>("host-key-prompt-withdrawn", (event) =>
        setPrompts((queue) => queue.filter((p) => p.prompt_id !== event.payload.prompt_id)),
      ),
      listen("hello-unavailable", () => setHelloMissing(true)),
      // Another device changed something: show it.
      listen("data-changed", () => void reload()),
    ];
    return () => {
      for (const unlisten of listeners) void unlisten.then((stop) => stop());
    };
  }, [reload, reloadRecent]);

  const connect = useCallback((host: Host) => {
    dispatch({ type: "open", tab: { id: crypto.randomUUID(), hostId: host.id, title: host.name } });
  }, []);

  const runAction = useCallback(
    (action: ShortcutAction) => {
      switch (action) {
        case "newTab":
          dispatch({ type: "openNew", id: crypto.randomUUID() });
          break;
        case "closeTab":
          dispatch({ type: "close", id: tabs.activeId });
          break;
        case "nextTab":
          dispatch({ type: "next" });
          break;
        case "prevTab":
          dispatch({ type: "prev" });
          break;
        case "palette":
          setPaletteOpen(true);
          break;
      }
    },
    [tabs.activeId],
  );
  const actionRef = useRef(runAction);
  actionRef.current = runAction;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inTerminal = e.target instanceof Element && e.target.closest(".terminal-host") !== null;
      const action = matchShortcut(e, inTerminal ? "terminal" : "app");
      if (!action) return;
      e.preventDefault();
      actionRef.current(action);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function paletteAction(id: PaletteActionId) {
    switch (id) {
      case "newTab":
        runAction("newTab");
        break;
      case "closeTab":
        runAction("closeTab");
        break;
      case "newHost":
        setEditor({ kind: "host" });
        break;
      case "newGroup":
        setEditor({ kind: "group" });
        break;
      case "importSshConfig":
        setEditor({ kind: "sshconfig" });
        break;
      case "openHosts":
      case "openKeys":
      case "openSettings":
        setPage(id === "openHosts" ? "hosts" : id === "openKeys" ? "keys" : "settings");
        dispatch({ type: "activate", id: HOME_ID });
        break;
    }
  }

  const askDelete = (message: string, action: () => Promise<void>) =>
    setConfirming({ message, confirmLabel: t("common.delete"), action });

  async function confirmed() {
    const pending = confirming;
    setConfirming(null);
    if (!pending) return;
    try {
      await pending.action();
      await reload();
    } catch (e) {
      setProblem(e);
    }
  }

  const done = () => {
    setEditor(null);
    void reload();
  };

  const activeTab = tabs.tabs.find((tab) => tab.id === tabs.activeId);

  return (
    <div className="shell">
      <TabStrip state={tabs} dispatch={dispatch} onNewTab={() => runAction("newTab")} />
      {vault && (
        <div className="banner">
          {vault.code === "master_key_missing"
            ? t("banner.vault.master_key_missing")
            : t("banner.vault.other", { reason: errorText(t, vault) })}
        </div>
      )}
      {problem !== null && (
        <div className="banner">
          <ErrorMessage error={problem} />
        </div>
      )}
      {helloMissing && !helloDismissed && (
        <div className="banner">
          {t("banner.hello")}{" "}
          <button className="link" onClick={() => setHelloDismissed(true)}>
            {t("common.dismiss")}
          </button>
        </div>
      )}
      <div className="content">
        {activeTab?.kind === "home" && (
          <div className="home">
            <NavSidebar page={page} onPage={setPage} />
            <main className="home-main">
              {page === "hosts" && (
                <HostsPage
                  groups={groups}
                  hosts={hosts}
                  onConnect={connect}
                  onEditHost={(host) => setEditor({ kind: "host", host })}
                  onDeleteHost={(host) => askDelete(t("hosts.deleteConfirm", { name: host.name }), () => deleteHost(host.id))}
                  onEditGroup={(group) => setEditor({ kind: "group", group })}
                  onDeleteGroup={(group) =>
                    askDelete(t("groups.deleteConfirm", { name: group.name }), () => deleteGroup(group.id))
                  }
                  onNewHost={() => setEditor({ kind: "host" })}
                  onNewGroup={() => setEditor({ kind: "group" })}
                  onImportSshConfig={() => setEditor({ kind: "sshconfig" })}
                  servers={servers}
                  keys={keys}
                  onMove={(kind: ItemKind, id: string, location: string | null) =>
                    void moveItem(kind, id, location).then(() => reload(), setProblem)
                  }
                />
              )}
              {page === "keys" && (
                <KeysPage
                  keys={keys}
                  onChanged={() => void reload()}
                  servers={servers}
                  onMove={(id: string, location: string | null) =>
                    void moveItem("key", id, location).then(() => reload(), setProblem)
                  }
                />
              )}
              {page === "settings" && <SettingsPage onDataChanged={() => void reload()} />}
              {(page === "snippets" || page === "forwarding" || page === "logs") && <PlaceholderPage page={page} />}
            </main>
          </div>
        )}
        {activeTab?.kind === "new" && (
          <NewTabPage
            hosts={hosts}
            groups={groups}
            recent={recent}
            onPick={(host) => dispatch({ type: "pick", id: activeTab.id, hostId: host.id, title: host.name })}
          />
        )}
        {/* Sessions stay mounted while hidden, so switching tabs never drops a connection. */}
        {tabs.tabs.map(
          (tab) =>
            tab.kind === "session" && (
              <div key={tab.id} className={tab.id === tabs.activeId ? "session-pane" : "session-pane hidden"}>
                <TerminalView
                  tab={tab}
                  active={tab.id === tabs.activeId}
                  onAttach={(sessionId) => dispatch({ type: "attach", id: tab.id, sessionId })}
                  onStatus={(status) => {
                    dispatch({ type: "status", id: tab.id, status });
                    if (status.state === "connected") reloadRecent();
                  }}
                />
                <SessionStatusLine status={tab.status} />
              </div>
            ),
        )}
      </div>

      {editor && (
        <Drawer onClose={() => setEditor(null)}>
          {editor.kind === "host" && (
            <HostForm host={editor.host} groups={groups} keys={keys} onDone={done} onCancel={() => setEditor(null)} />
          )}
          {editor.kind === "group" && (
            <GroupForm group={editor.group} keys={keys} onDone={done} onCancel={() => setEditor(null)} />
          )}
          {editor.kind === "sshconfig" && <SshConfigImport onDone={done} onCancel={() => setEditor(null)} />}
        </Drawer>
      )}

      {paletteOpen && (
        <CommandPalette
          hosts={hosts}
          groups={groups}
          recent={recent}
          onClose={() => setPaletteOpen(false)}
          onHost={connect}
          onAction={paletteAction}
        />
      )}

      {confirming && (
        <ConfirmDialog
          message={confirming.message}
          confirmLabel={confirming.confirmLabel}
          onConfirm={() => void confirmed()}
          onCancel={() => setConfirming(null)}
        />
      )}

      {prompts[0] && (
        // A new key per question remounts the dialog, so the focus starts on "reject" again.
        <HostKeyDialog
          key={prompts[0].prompt_id}
          prompt={prompts[0]}
          onAnswer={(accept) => {
            const answered = prompts[0].prompt_id;
            void answerHostKey(answered, accept);
            setPrompts((queue) => queue.filter((p) => p.prompt_id !== answered));
          }}
        />
      )}
    </div>
  );
}
