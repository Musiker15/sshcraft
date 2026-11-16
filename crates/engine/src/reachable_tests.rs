//! `host.reachable`: a TCP connect to each host's address and port, nothing more.

use serde_json::json;
use sshcraft_model::model::{ConnectionSettings, Host, HostId};

use crate::{Caller, Session, SessionConfig};

fn host(name: &str, port: u16, jump: Option<HostId>) -> Host {
    Host {
        id: HostId::new(),
        name: name.into(),
        group_id: None,
        settings: ConnectionSettings { address: Some("127.0.0.1".into()), port: Some(port), jump_hosts: jump.map(|j| vec![j]), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    }
}

async fn save(session: &Session, host: &Host) {
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
}

#[tokio::test]
async fn an_open_port_is_reachable_and_a_closed_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let open = host("open", listener.local_addr().unwrap().port(), None);
    let closed_port = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let closed = host("closed", closed_port, None);
    let routed = host("routed", listener.local_addr().unwrap().port(), Some(open.id));
    for h in [&open, &closed, &routed] {
        save(&session, h).await;
    }
    let out = session.execute(Caller::User, "host.reachable", json!({ "ids": [open.id, closed.id, routed.id, HostId::new()] })).await.unwrap();
    let state = |id: HostId| out.as_array().unwrap().iter().find(|r| r["id"] == json!(id)).map(|r| r["reachable"].clone());
    assert_eq!(state(open.id), Some(json!(true)));
    assert_eq!(state(closed.id), Some(json!(false)));
    // Behind a jump host the address is not ours to reach: unknown.
    assert_eq!(state(routed.id), Some(json!(null)));
    assert_eq!(out.as_array().unwrap().len(), 3, "an unknown id is left out");
}

#[tokio::test]
async fn too_many_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let ids: Vec<HostId> = (0..501).map(|_| HostId::new()).collect();
    assert!(session.execute(Caller::User, "host.reachable", json!({ "ids": ids })).await.is_err());
}

#[tokio::test]
async fn agents_cannot_probe_hosts() {
    use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentPolicy};
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    // Even with every right granted: the command has no agent access.
    for category in [AgentCategory::List, AgentCategory::Connect, AgentCategory::Edit] {
        AgentPolicy::set_level(&session.store(), category, AgentLevel::Allow).unwrap();
    }
    let conn = session.agent_connection("claude-code", crate::caller::Via::Mcp).unwrap();
    assert!(session.execute(conn.caller(), "host.reachable", json!({ "ids": [] })).await.is_err());
}
