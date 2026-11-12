import { useCallback, useEffect, useReducer, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  answerHostKey,
  deleteGroup,
  deleteHost,
  listGroups,
  listHosts,
  listKeys,
  helloStatus,
  vaultStatus,
  type Group,
  type Host,
  type HostKeyPrompt,
  type KeyInfo,
  type VaultStatus,
} from "./api";
import { errorText } from "./i18n/errors";
import { useT } from "./i18n/i18n";
import { GroupForm } from "./components/GroupForm";
import { HostForm } from "./components/HostForm";
import { HostKeyDialog } from "./components/HostKeyDialog";
import { KeyManager } from "./components/KeyManager";
import { Sidebar } from "./components/Sidebar";
import { SshConfigImport } from "./components/SshConfigImport";
import { TerminalTabs } from "./components/TerminalTabs";
import { initialTabs, tabsReducer } from "./state/tabs";

type Editor =
  | { kind: "host"; host?: Host }
  | { kind: "group"; group?: Group }
  | { kind: "keys" }
  | { kind: "sshconfig" }
  | null;

function App() {
  const t = useT();
  const [groups, setGroups] = useState<Group[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editor, setEditor] = useState<Editor>(null);
  const [prompts, setPrompts] = useState<HostKeyPrompt[]>([]);
  const [tabs, dispatch] = useReducer(tabsReducer, initialTabs);
  const [helloMissing, setHelloMissing] = useState(false);
  const [helloDismissed, setHelloDismissed] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [g, h, k] = await Promise.all([listGroups(), listHosts(), listKeys()]);
      setGroups(g);
      setHosts(h);
      setKeys(k);
      setLoadError(null);
    } catch (e) {
      setLoadError(errorText(t, e));
    }
  }, [t]);

  useEffect(() => {
    void reload();
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
    ];
    return () => {
      for (const unlisten of listeners) void unlisten.then((stop) => stop());
    };
  }, [reload]);

  function connect(host: Host) {
    dispatch({ type: "open", tab: { id: crypto.randomUUID(), hostId: host.id, title: host.name } });
  }

  async function confirmDelete(message: string, action: () => Promise<void>) {
    if (!confirm(message)) return;
    try {
      await action();
      await reload();
    } catch (e) {
      alert(errorText(t, e));
    }
  }

  const done = () => {
    setEditor(null);
    void reload();
  };

  return (
    <div className="app">
      <Sidebar
        groups={groups}
        hosts={hosts}
        onConnect={connect}
        onEditHost={(host) => setEditor({ kind: "host", host })}
        onDeleteHost={(host) => confirmDelete(`Host "${host.name}" löschen?`, () => deleteHost(host.id))}
        onEditGroup={(group) => setEditor({ kind: "group", group })}
        onDeleteGroup={(group) =>
          confirmDelete(`Gruppe "${group.name}" und alle ihre Hosts löschen?`, () => deleteGroup(group.id))
        }
        onNewHost={() => setEditor({ kind: "host" })}
        onNewGroup={() => setEditor({ kind: "group" })}
        onKeys={() => setEditor({ kind: "keys" })}
        onSshConfig={() => setEditor({ kind: "sshconfig" })}
      />
      <main className="main">
        {vault && (
          <div className="banner">
            {vault.code === "master_key_missing"
              ? t("banner.vault.master_key_missing")
              : t("banner.vault.other", { reason: errorText(t, vault) })}
          </div>
        )}
        {loadError && <div className="banner">{loadError}</div>}
        {helloMissing && !helloDismissed && (
          <div className="banner">
            {t("banner.hello")}{" "}
            <button className="link" onClick={() => setHelloDismissed(true)}>
              {t("common.dismiss")}
            </button>
          </div>
        )}
        <TerminalTabs state={tabs} dispatch={dispatch} />
      </main>

      {editor && (
        <div className="overlay">
          {editor.kind === "host" && (
            <HostForm host={editor.host} groups={groups} keys={keys} onDone={done} onCancel={() => setEditor(null)} />
          )}
          {editor.kind === "group" && (
            <GroupForm group={editor.group} keys={keys} onDone={done} onCancel={() => setEditor(null)} />
          )}
          {editor.kind === "sshconfig" && <SshConfigImport onDone={done} onCancel={() => setEditor(null)} />}
          {editor.kind === "keys" && <KeyManager keys={keys} onChanged={reload} onClose={() => setEditor(null)} />}
        </div>
      )}

      {prompts[0] && (
        <HostKeyDialog
          prompt={prompts[0]}
          onAnswer={(accept) => {
            void answerHostKey(prompts[0].prompt_id, accept);
            setPrompts((queue) => queue.slice(1));
          }}
        />
      )}
    </div>
  );
}

export default App;
