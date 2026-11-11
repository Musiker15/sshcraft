import { useCallback, useEffect, useReducer, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  answerHostKey,
  deleteGroup,
  deleteHost,
  errorMessage,
  listGroups,
  listHosts,
  listKeys,
  vaultStatus,
  type Group,
  type Host,
  type HostKeyPrompt,
  type KeyInfo,
} from "./api";
import { GroupForm } from "./components/GroupForm";
import { HostForm } from "./components/HostForm";
import { HostKeyDialog } from "./components/HostKeyDialog";
import { KeyManager } from "./components/KeyManager";
import { Sidebar } from "./components/Sidebar";
import { TerminalTabs } from "./components/TerminalTabs";
import { initialTabs, tabsReducer } from "./state/tabs";

type Editor = { kind: "host"; host?: Host } | { kind: "group"; group?: Group } | { kind: "keys" } | null;

function App() {
  const [groups, setGroups] = useState<Group[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [vaultError, setVaultError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editor, setEditor] = useState<Editor>(null);
  const [prompts, setPrompts] = useState<HostKeyPrompt[]>([]);
  const [tabs, dispatch] = useReducer(tabsReducer, initialTabs);

  const reload = useCallback(async () => {
    try {
      const [g, h, k] = await Promise.all([listGroups(), listHosts(), listKeys()]);
      setGroups(g);
      setHosts(h);
      setKeys(k);
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
    void vaultStatus().then((s) => setVaultError(s.ok ? null : s.error));
    const unlisten = listen<HostKeyPrompt>("host-key-prompt", (event) =>
      setPrompts((queue) => [...queue, event.payload]),
    );
    return () => {
      void unlisten.then((stop) => stop());
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
      alert(errorMessage(e));
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
      />
      <main className="main">
        {vaultError && (
          <div className="banner">
            Die gespeicherten Schlüssel lassen sich nicht entschlüsseln: Der Hauptschlüssel fehlt in der
            Windows-Anmeldeinformationsverwaltung (Eintrag „de.musiker15.stollen“). Stellst du ihn wieder her und
            startest Stollen neu, sind alle Schlüssel wieder da. ({vaultError})
          </div>
        )}
        {loadError && <div className="banner">{loadError}</div>}
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
