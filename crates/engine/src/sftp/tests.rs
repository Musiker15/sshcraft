use serde_json::{Value, json};

use crate::caller::{AgentName, Caller, ConnId, Via};
use crate::error::{EngineError, ErrorCode};
use crate::{Session, SessionConfig};

struct Fixture {
    session: Session,
    server: sshcraft_ssh::testing::TestServer,
    root: tempfile::TempDir,
    id: u64,
    _dir: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::open(SessionConfig::for_tests(dir.path())).unwrap().0;
    let (server, host) = crate::testing::trusted_test_host(&session).await;
    let root = tempfile::tempdir().unwrap();
    server.serve_sftp(root.path());
    let opened = session.execute(Caller::User, "sftp.open", json!({ "host_id": host })).await.unwrap();
    assert_eq!(opened["home"], "/");
    let id = opened["id"].as_u64().unwrap();
    Fixture { session, server, root, id, _dir: dir }
}

async fn run(f: &Fixture, id: &str, params: Value) -> Result<Value, EngineError> {
    f.session.execute(Caller::User, id, params).await
}

fn code(e: EngineError) -> ErrorCode {
    match e {
        EngineError::Failed { code, .. } => code,
        other => panic!("not a failure: {other:?}"),
    }
}

fn names(listing: &Value) -> Vec<String> {
    let mut names: Vec<String> = listing["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_without_sftp_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::open(SessionConfig::for_tests(dir.path())).unwrap().0;
    let (_server, host) = crate::testing::trusted_test_host(&session).await;
    let err = session.execute(Caller::User, "sftp.open", json!({ "host_id": host })).await.unwrap_err();
    assert_eq!(code(err), ErrorCode::SftpUnavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_creates_renames_and_changes_permissions() {
    let f = fixture().await;
    std::fs::write(f.root.path().join("a.txt"), b"abc").unwrap();
    run(&f, "sftp.mkdir", json!({ "id": f.id, "dir": "/", "name": "docs" })).await.unwrap();
    let listing = run(&f, "sftp.list", json!({ "id": f.id, "path": "/docs/.." })).await.unwrap();
    assert_eq!(listing["path"], "/");
    assert_eq!(names(&listing), ["a.txt", "docs"]);

    let renamed = run(&f, "sftp.rename", json!({ "id": f.id, "path": "/a.txt", "name": "b.txt" })).await.unwrap();
    assert_eq!(renamed["path"], "/b.txt");
    assert!(f.root.path().join("b.txt").is_file());
    let taken = run(&f, "sftp.rename", json!({ "id": f.id, "path": "/b.txt", "name": "docs" })).await.unwrap_err();
    assert_eq!(code(taken), ErrorCode::AlreadyExists);

    run(&f, "sftp.chmod", json!({ "id": f.id, "path": "/b.txt", "mode": 0o600 })).await.unwrap();
    assert_eq!(f.server.sftp_mode("/b.txt"), Some(0o600));
    assert!(run(&f, "sftp.chmod", json!({ "id": f.id, "path": "/b.txt", "mode": 0o10000 })).await.is_err());
}

/// A name the user types for a folder or a rename cannot climb out of its folder.
#[tokio::test(flavor = "multi_thread")]
async fn typed_names_cannot_leave_their_folder() {
    let f = fixture().await;
    std::fs::create_dir(f.root.path().join("in")).unwrap();
    std::fs::write(f.root.path().join("in/x"), b"").unwrap();
    for bad in ["..", "../out", "a/b", ""] {
        let err = run(&f, "sftp.mkdir", json!({ "id": f.id, "dir": "/in", "name": bad })).await.unwrap_err();
        assert_eq!(code(err), ErrorCode::NameInvalid, "{bad:?}");
        let err = run(&f, "sftp.rename", json!({ "id": f.id, "path": "/in/x", "name": bad })).await.unwrap_err();
        assert_eq!(code(err), ErrorCode::NameInvalid, "{bad:?}");
    }
    assert!(f.root.path().join("in/x").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn deletes_folders_with_everything_in_them_after_counting() {
    let f = fixture().await;
    let tree = f.root.path().join("tree/sub");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join("one"), b"12345").unwrap();
    std::fs::write(f.root.path().join("tree/two"), b"1").unwrap();
    std::fs::write(f.root.path().join("keep"), b"").unwrap();
    let count = run(&f, "sftp.count", json!({ "id": f.id, "paths": ["/tree"] })).await.unwrap();
    assert_eq!(count, json!({ "files": 2, "dirs": 2, "bytes": 6 }));
    let deleted = run(&f, "sftp.delete", json!({ "id": f.id, "paths": ["/tree"] })).await.unwrap();
    assert_eq!(deleted["deleted"], 4);
    assert!(!f.root.path().join("tree").exists());
    assert!(f.root.path().join("keep").exists());
    let root = run(&f, "sftp.delete", json!({ "id": f.id, "paths": ["/x/.."] })).await.unwrap_err();
    assert_eq!(code(root), ErrorCode::NameInvalid, "/ is never deleted");
}

/// Deleting a link to a folder removes the link, never what it points to.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_link_leaves_its_target() {
    let f = fixture().await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("precious"), b"x").unwrap();
    std::os::unix::fs::symlink(outside.path(), f.root.path().join("link")).unwrap();
    run(&f, "sftp.delete", json!({ "id": f.id, "paths": ["/link"] })).await.unwrap();
    assert!(outside.path().join("precious").exists());
    assert!(f.root.path().join("link").symlink_metadata().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_have_no_access_to_sftp() {
    let f = fixture().await;
    let agent = Caller::Agent { name: AgentName::new("claude-code").unwrap(), via: Via::Mcp, conn: ConnId(1) };
    for (id, params) in [
        ("sftp.list", json!({ "id": f.id, "path": "/" })),
        ("sftp.delete", json!({ "id": f.id, "paths": ["/x"] })),
        ("sftp.local_list", json!({})),
    ] {
        assert!(f.session.execute(agent.clone(), id, params).await.is_err(), "{id}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_side_lists_a_folder_and_starts_at_home() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("here.txt"), b"12").unwrap();
    std::fs::create_dir(local.path().join("sub")).unwrap();
    let listing = run(&f, "sftp.local_list", json!({ "path": local.path() })).await.unwrap();
    assert_eq!(names(&listing), ["here.txt", "sub"]);
    assert!(run(&f, "sftp.local_list", json!({ "path": "relative" })).await.is_err());
    if crate::cmd::sftp::local_home().is_some() {
        assert!(run(&f, "sftp.local_list", json!({})).await.is_ok());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_is_gone() {
    let f = fixture().await;
    run(&f, "sftp.close", json!({ "id": f.id })).await.unwrap();
    assert!(run(&f, "sftp.list", json!({ "id": f.id, "path": "/" })).await.is_err());
    assert!(run(&f, "sftp.close", json!({ "id": f.id })).await.is_err());
}
