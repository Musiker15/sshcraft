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
  itemShares,
  listKeys,
  identitiesList,
  localShells,
  serialRemember,
  sessionWrite,
  workspaceTemplates,
  deleteWorkspaceTemplate,
  type WorkspaceTemplate,
  type SerialSettings,
  type SessionStatus,
  type SessionTarget,
  type ShellCommand,
  answerAuth,
  type AuthPrompt,
  type Identity,
  listRecent,
  listServers,
  moveItem,
  vaultStatus,
  type Group,
  type Host,
  type HostKeyPrompt,
  type ItemKind,
  type KeyInfo,
  type KeyRef,
  type AgentKey,
  listAgentKeys,
  type ItemShare,
  type ServerView,
  type VaultStatus,
} from "../api";
import { GroupForm } from "../components/GroupForm";
import { HostForm } from "../components/HostForm";
import { AuthPromptDialog } from "../components/AuthPromptDialog";
import { HostKeyDialog } from "../components/HostKeyDialog";
import { ExportDialog } from "../components/ExportDialog";
import { ImportDialog } from "../components/ImportDialog";
import { InstallKeyDialog } from "../components/KeyDialogs";
import { WorkspaceView } from "../components/WorkspaceView";
import { TemplateDialog } from "../components/TemplateDialog";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { HostsPage } from "../pages/HostsPage";
import { ForwardingPage } from "../pages/ForwardingPage";
import { KeysPage } from "../pages/KeysPage";
import { NewTabPage } from "../pages/NewTabPage";
import { CommandPalette } from "../palette/CommandPalette";
import type { PaletteActionId } from "../palette/rank";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { SettingsPage } from "../pages/SettingsPage";
import { ShareDialog, type ShareTarget } from "../pages/sync/ShareDialog";
import {
  HOME_ID,
  broadcastTargets,
  initialTabs,
  isPicker,
  tabsReducer,
  targetTitle,
  type Pane,
  type SessionTab,
} from "../state/tabs";
import { ConfirmDialog } from "./ConfirmDialog";
import { Drawer } from "./Drawer";
import { ErrorMessage } from "./ErrorMessage";
import { NavSidebar, type Page } from "./NavSidebar";
import { matchShortcut, resolveBindings, type ShortcutAction } from "./shortcuts";
import { useSettings } from "../settings/SettingsProvider";
import { TabStrip } from "./TabStrip";

type Editor =
  | { kind: "host"; host?: Host; group?: string | null }
  | { kind: "group"; group?: Group; parent?: string | null }
  | { kind: "import" }
  | { kind: "export" }
  | null;
type Confirm = { message: string; confirmLabel: string; action: () => Promise<void> } | null;

export function AppShell() {
  const t = useT();
  const [groups, setGroups] = useState<Group[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [shells, setShells] = useState<ShellCommand[]>([]);
  const [authPrompts, setAuthPrompts] = useState<AuthPrompt[]>([]);
  const [servers, setServers] = useState<ServerView[]>([]);
  const [shares, setShares] = useState<ItemShare[]>([]);
  const [sharing, setSharing] = useState<{ target: ShareTarget; server: ServerView } | null>(null);
  const [installing, setInstalling] = useState<{ host?: string; key?: KeyRef } | null>(null);
  const [agentKeys, setAgentKeys] = useState<AgentKey[]>([]);
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [problem, setProblem] = useState<unknown>(null);
  const [editor, setEditor] = useState<Editor>(null);
  const [confirming, setConfirming] = useState<Confirm>(null);
  const [prompts, setPrompts] = useState<HostKeyPrompt[]>([]);
  const [tabs, dispatch] = useReducer(tabsReducer, initialTabs);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const [page, setPage] = useState<Page>("hosts");
  const [helloMissing, setHelloMissing] = useState(false);
  const [helloDismissed, setHelloDismissed] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [templates, setTemplates] = useState<WorkspaceTemplate[]>([]);
  const [templateFor, setTemplateFor] = useState<SessionTab | null>(null);
  /** Panes of the last opened template left out because their host is gone. */
  const [skipped, setSkipped] = useState(0);

  const reloadTemplates = useCallback(() => {
    workspaceTemplates().then((list) => setTemplates(list ?? []), () => setTemplates([]));
  }, []);

  /** "Add to authorized_keys": the agent's keys are offered too, fetched when it opens. */
  const openInstall = useCallback((start: { host?: string; key?: KeyRef }) => {
    setInstalling(start);
    listAgentKeys().then(setAgentKeys, () => setAgentKeys([]));
  }, []);

  const reloadRecent = useCallback(() => {
    listRecent(20).then(setRecent, () => {});
  }, []);

  const reload = useCallback(async () => {
    try {
      const [g, h, k, s, sh, ids] = await Promise.all([
        listGroups(),
        listHosts(),
        listKeys(),
        listServers(),
        itemShares(),
        identitiesList(),
      ]);
      setIdentities(ids ?? []);
      setShares(sh ?? []);
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
    reloadTemplates();
    localShells().then(setShells, () => setShells([]));
    void vaultStatus().then((s) => setVault(s.ok ? null : s));
    // Hello is only checked when a stored key is first used; the core reports the outcome.
    void helloStatus().then((s) => setHelloMissing(s.missing));
    const listeners = [
      listen<HostKeyPrompt>("host-key-prompt", (event) => setPrompts((queue) => [...queue, event.payload])),
      listen<AuthPrompt>("auth-prompt", (event) => setAuthPrompts((queue) => [...queue, event.payload])),
      // The tab that asked was closed: its question must not stay on screen (both kinds come
      // under this name).
      listen<{ prompt_id: number }>("host-key-prompt-withdrawn", (event) => {
        setAuthPrompts((queue) => queue.filter((p) => p.prompt_id !== event.payload.prompt_id));
        setPrompts((queue) => queue.filter((p) => p.prompt_id !== event.payload.prompt_id));
      }),
      listen("hello-unavailable", () => setHelloMissing(true)),
      // Another device changed something: show it.
      listen("data-changed", () => void reload()),
    ];
    return () => {
      for (const unlisten of listeners) void unlisten.then((stop) => stop());
    };
  }, [reload, reloadRecent, reloadTemplates]);

  /** Opens a template as a new tab; panes whose host was deleted are left out. */
  const openTemplate = useCallback(
    (template: WorkspaceTemplate) => {
      const known = new Set(hosts.map((h) => h.id));
      const panes: Pane[] = template.panes.flatMap((p): Pane[] => {
        const target = p.target;
        if (target.kind === "host" && !known.has(target.host_id)) return [];
        return [
          {
            id: crypto.randomUUID(),
            target: target.kind === "local" ? { kind: "local", shell: target.shell, cwd: p.cwd ?? null } : target,
            title: p.title,
            status: { state: "connecting" },
            activity: "none",
            startup: { cwd: p.cwd ?? null, command: p.command ?? null },
          },
        ];
      });
      setSkipped(template.panes.length - panes.length);
      // All panes kept: the stored grid fits; otherwise it starts equal.
      const complete = panes.length === template.panes.length;
      dispatch({
        type: "openTemplate",
        tabId: crypto.randomUUID(),
        name: template.name,
        layout: template.layout,
        colSizes: complete ? template.col_sizes : [],
        rowSizes: complete ? template.row_sizes : [],
        panes,
      });
    },
    [hosts],
  );

  const connect = useCallback((host: Host) => {
    dispatch({ type: "open", tab: { id: crypto.randomUUID(), target: { kind: "host", host_id: host.id }, title: host.name } });
  }, []);

  const activeSession = tabs.tabs.find(
    (tab): tab is SessionTab => tab.id === tabs.activeId && tab.kind === "session",
  );

  /** Typed input goes to its own session, with broadcast on to every session of the tab. */
  const sendInput = useCallback((paneId: string, data: string, report: boolean) => {
    const tab = tabsRef.current.tabs.find(
      (t): t is SessionTab => t.kind === "session" && t.panes.some((p) => p.id === paneId),
    );
    const pane = tab?.panes.find((p) => p.id === paneId);
    if (!tab || !pane || isPicker(pane)) return;
    const targets = tab.broadcast && !report ? broadcastTargets(tab) : pane.sessionId ? [pane.sessionId] : [];
    for (const id of targets) void sessionWrite(id, data).catch(() => {});
  }, []);

  const paneStatus = useCallback(
    (paneId: string, status: SessionStatus) => {
      dispatch({ type: "status", paneId, status });
      if (status.state === "connected") reloadRecent();
    },
    [reloadRecent],
  );

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
        case "splitPane":
          if (activeSession) dispatch({ type: "split", tabId: activeSession.id, paneId: crypto.randomUUID() });
          break;
        case "closePane":
          if (activeSession) dispatch({ type: "closePane", paneId: activeSession.focusedId });
          break;
        case "nextPane":
          dispatch({ type: "nextPane" });
          break;
        case "prevPane":
          dispatch({ type: "prevPane" });
          break;
        case "toggleLayout":
          if (activeSession)
            dispatch({
              type: "setLayout",
              tabId: activeSession.id,
              layout: activeSession.layout === "focus" ? "split" : "focus",
            });
          break;
        case "toggleBroadcast":
          if (activeSession) dispatch({ type: "toggleBroadcast", tabId: activeSession.id });
          break;
      }
    },
    [tabs.activeId, activeSession],
  );
  const actionRef = useRef(runAction);
  actionRef.current = runAction;
  const { settings } = useSettings();
  const bindings = useRef(resolveBindings(settings.shortcuts));
  bindings.current = resolveBindings(settings.shortcuts);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inTerminal = e.target instanceof Element && e.target.closest(".terminal-host") !== null;
      const action = matchShortcut(e, inTerminal ? "terminal" : "app", bindings.current);
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
      case "importHosts":
        setEditor({ kind: "import" });
        break;
      case "exportHosts":
        setEditor({ kind: "export" });
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
      <TabStrip
        state={tabs}
        dispatch={dispatch}
        onNewTab={() => runAction("newTab")}
        onSplit={() => runAction("splitPane")}
        onSaveTemplate={setTemplateFor}
      />
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
      {skipped > 0 && (
        <div className="banner">
          {t("workspaces.skipped", { count: skipped })}{" "}
          <button className="link" onClick={() => setSkipped(0)}>
            {t("common.dismiss")}
          </button>
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
                  onNewHost={(group) => setEditor({ kind: "host", group })}
                  onNewGroup={(parent) => setEditor({ kind: "group", parent })}
                  onImportSshConfig={() => setEditor({ kind: "import" })}
                  onExport={() => setEditor({ kind: "export" })}
                  servers={servers}
                  keys={keys}
                  onMove={(kind: ItemKind, id: string, location: string | null) =>
                    void moveItem(kind, id, location).then(() => reload(), setProblem)
                  }
                  shares={shares}
                  onShare={(target, server) => setSharing({ target, server })}
                  onInstallKey={(host) => openInstall({ host: host.id })}
                />
              )}
              {page === "keys" && (
                <KeysPage
                  keys={keys}
                  identities={identities}
                  onChanged={() => void reload()}
                  servers={servers}
                  onMove={(id: string, location: string | null) =>
                    void moveItem("key", id, location).then(() => reload(), setProblem)
                  }
                  onInstall={(key) => openInstall({ key })}
                />
              )}
              {page === "settings" && <SettingsPage onDataChanged={() => void reload()} />}
              {page === "forwarding" && <ForwardingPage hosts={hosts} onChanged={() => void reload()} />}
              {(page === "snippets" || page === "logs") && <PlaceholderPage page={page} />}
            </main>
          </div>
        )}
        {activeTab?.kind === "new" && (
          <NewTabPage
            hosts={hosts}
            groups={groups}
            recent={recent}
            onPick={(host) =>
              dispatch({ type: "pick", id: activeTab.id, target: { kind: "host", host_id: host.id }, title: host.name })
            }
            templates={templates}
            onTemplate={(template) => {
              dispatch({ type: "close", id: activeTab.id });
              openTemplate(template);
            }}
            onDeleteTemplate={(template) =>
              askDelete(t("workspaces.deleteConfirm", { name: template.name }), async () => {
                await deleteWorkspaceTemplate(template.id);
                reloadTemplates();
              })
            }
            shells={shells}
            onShell={(shell) => {
              const target: SessionTarget = { kind: "local", shell };
              dispatch({ type: "pick", id: activeTab.id, target, title: targetTitle(target) });
            }}
            onSerial={(settings: SerialSettings) => {
              void serialRemember(settings).catch(() => {});
              const target: SessionTarget = { kind: "serial", settings };
              dispatch({ type: "pick", id: activeTab.id, target, title: targetTitle(target) });
            }}
          />
        )}
        {/* Panes stay mounted while hidden, so switching tabs never drops a connection. */}
        <WorkspaceView
          state={tabs}
          dispatch={dispatch}
          onInput={sendInput}
          onStatus={paneStatus}
          renderPicker={(paneId) => (
            <NewTabPage
              hosts={hosts}
              groups={groups}
              recent={recent}
              onPick={(host) =>
                dispatch({ type: "pickPane", paneId, target: { kind: "host", host_id: host.id }, title: host.name })
              }
              shells={shells}
              onShell={(shell) => {
                const target: SessionTarget = { kind: "local", shell };
                dispatch({ type: "pickPane", paneId, target, title: targetTitle(target) });
              }}
              onSerial={(settings: SerialSettings) => {
                void serialRemember(settings).catch(() => {});
                const target: SessionTarget = { kind: "serial", settings };
                dispatch({ type: "pickPane", paneId, target, title: targetTitle(target) });
              }}
            />
          )}
        />
      </div>

      {editor && (
        <Drawer onClose={() => setEditor(null)}>
          {editor.kind === "host" && (
            <HostForm
              host={editor.host}
              group={editor.group}
              groups={groups}
              keys={keys}
              identities={identities}
              hosts={hosts}
              onDone={done}
              onCancel={() => setEditor(null)}
            />
          )}
          {editor.kind === "group" && (
            <GroupForm
              group={editor.group}
              parent={editor.parent}
              groups={groups}
              keys={keys}
              identities={identities}
              hosts={hosts}
              onDone={done}
              onCancel={() => setEditor(null)}
            />
          )}
          {editor.kind === "import" && <ImportDialog servers={servers} onDone={done} onCancel={done} />}
          {editor.kind === "export" && <ExportDialog groups={groups} servers={servers} onDone={() => setEditor(null)} />}
        </Drawer>
      )}

      {installing && (
        <Drawer onClose={() => setInstalling(null)}>
          <InstallKeyDialog
            hosts={hosts}
            keys={keys}
            agentKeys={agentKeys}
            host={installing.host}
            keyRef={installing.key}
            onClose={() => setInstalling(null)}
          />
        </Drawer>
      )}

      {templateFor && (
        <Drawer onClose={() => setTemplateFor(null)}>
          <TemplateDialog
            tab={templateFor}
            existing={templates}
            onDone={() => {
              setTemplateFor(null);
              reloadTemplates();
            }}
            onCancel={() => setTemplateFor(null)}
          />
        </Drawer>
      )}

      {sharing && (
        <Drawer onClose={() => setSharing(null)}>
          <ShareDialog
            server={sharing.server}
            target={sharing.target}
            hosts={hosts}
            onDone={() => {
              setSharing(null);
              void reload();
            }}
            onCancel={() => setSharing(null)}
          />
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
          shells={shells}
          onShell={(shell) => {
            const target: SessionTarget = { kind: "local", shell };
            dispatch({ type: "open", tab: { id: crypto.randomUUID(), target, title: targetTitle(target) } });
          }}
          templates={templates}
          onTemplate={openTemplate}
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

      {authPrompts[0] && !prompts[0] && (
        <AuthPromptDialog
          key={authPrompts[0].prompt_id}
          prompt={authPrompts[0]}
          onAnswer={(answers) => {
            const answered = authPrompts[0].prompt_id;
            void answerAuth(answered, answers);
            setAuthPrompts((queue) => queue.filter((p) => p.prompt_id !== answered));
          }}
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
