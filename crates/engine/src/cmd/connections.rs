//! Every connection SSHCraft makes, or offers, by itself, for Settings › Connections: the sync
//! servers, the password managers it asks, the SSH agent, the port AI agents connect on. There
//! is no telemetry and no update check; the list says so.

use serde::Serialize;
use serde_json::Value;
use sshcraft_model::model::ConnectionSettings;

use super::{CommandError, CommandSpec, query, sync_run, to_value};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![query("app.connections", "Connections SSHCraft makes by itself", "none", run_list)]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    /// `sync`, `password_manager`, `wake_on_lan`, `port_knock`, `connect_program`, `ssh_agent`,
    /// `agent_port`, `update_check`.
    pub kind: &'static str,
    /// Where to: an address, a program, a pipe; `none` for what SSHCraft never does.
    pub target: String,
    /// The server's name or the manager's, for the text the interface makes.
    pub name: String,
    /// How many hosts or groups use it, where that counts.
    pub count: usize,
    /// Whether it happens now (switched on).
    pub active: bool,
}

fn run_list(ctx: Ctx, _params: Value) -> Out {
    sync_run(move || {
        let store = ctx.session.store();
        let settings = ctx.session.settings().get().clone();
        let mut out = Vec::new();
        for server in store.servers()? {
            out.push(Connection {
                kind: "sync",
                target: server.origin.clone(),
                name: server.name.clone(),
                count: 0,
                active: server.sync_enabled && !server.revoked,
            });
        }
        let hosts = store.list_hosts()?;
        let groups = store.list_groups()?;
        let levels: Vec<&ConnectionSettings> = hosts.iter().map(|h| &h.settings).chain(groups.iter().map(|g| &g.defaults)).collect();
        let mut managers: Vec<(sshcraft_model::secret_ref::Manager, usize)> = Vec::new();
        for r in levels.iter().filter_map(|s| s.password_ref.as_ref()) {
            match managers.iter_mut().find(|(m, _)| *m == r.manager) {
                Some((_, n)) => *n += 1,
                None => managers.push((r.manager, 1)),
            }
        }
        for (manager, count) in managers {
            out.push(Connection {
                kind: "password_manager",
                target: manager.program().to_owned(),
                name: manager.label().to_owned(),
                count,
                active: true,
            });
        }
        // Steps before and after connecting that reach out by themselves.
        let mut wake = 0;
        let mut knock = 0;
        let mut program = 0;
        for s in &levels {
            let steps: Vec<&sshcraft_model::hooks::ConnectStep> = [&s.before_connect, &s.after_disconnect].into_iter().flatten().flatten().collect();
            wake += usize::from(steps.iter().any(|st| matches!(st, sshcraft_model::hooks::ConnectStep::WakeOnLan { .. })));
            knock += usize::from(steps.iter().any(|st| matches!(st, sshcraft_model::hooks::ConnectStep::Knock { .. })));
            program += usize::from(steps.iter().any(|st| matches!(st, sshcraft_model::hooks::ConnectStep::Command { .. })));
        }
        for (kind, count, target) in
            [("wake_on_lan", wake, "UDP broadcast"), ("port_knock", knock, "TCP"), ("connect_program", program, "local program")]
        {
            if count > 0 {
                out.push(Connection { kind, target: target.to_owned(), name: String::new(), count, active: true });
            }
        }
        out.push(Connection {
            kind: "resource_bar",
            target: "SSH".to_owned(),
            name: String::new(),
            count: 0,
            active: settings.terminal.resource_bar,
        });
        out.push(Connection {
            kind: "reachability",
            target: "TCP".to_owned(),
            name: String::new(),
            count: 0,
            active: settings.appearance.show_reachability,
        });
        out.push(Connection {
            kind: "ssh_agent",
            target: if cfg!(windows) { r"\.\pipe\openssh-ssh-agent".to_owned() } else { "SSH_AUTH_SOCK".to_owned() },
            name: String::new(),
            count: 0,
            active: true,
        });
        out.push(Connection {
            kind: "agent_port",
            target: format!("127.0.0.1:{}", settings.integrations.agent_port),
            name: String::new(),
            count: 0,
            active: settings.integrations.allow_agent_control,
        });
        out.push(Connection { kind: "update_check", target: "none".to_owned(), name: String::new(), count: 0, active: false });
        to_value(&out)
    })
}
