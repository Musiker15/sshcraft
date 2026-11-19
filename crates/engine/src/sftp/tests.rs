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
    for (id, params) in
        [("sftp.list", json!({ "id": f.id, "path": "/" })), ("sftp.delete", json!({ "id": f.id, "paths": ["/x"] })), ("sftp.local_list", json!({}))]
    {
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

/// Waits until transfer `id` ended and gives its view.
async fn ended(f: &Fixture, id: &Value) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let all = run(f, "sftp.transfers", json!({})).await.unwrap();
        let job = all.as_array().unwrap().iter().find(|j| &j["id"] == id).cloned().unwrap();
        let state = job["state"]["kind"].as_str().unwrap().to_owned();
        if state != "queued" && state != "running" {
            return job;
        }
        assert!(std::time::Instant::now() < deadline, "the transfer did not end: {job}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn tree(at: &std::path::Path) {
    std::fs::create_dir_all(at.join("project/src")).unwrap();
    std::fs::write(at.join("project/readme.md"), b"Hallo Welt").unwrap();
    let big: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(at.join("project/src/data.bin"), &big).unwrap();
}

fn same_tree(a: &std::path::Path, b: &std::path::Path) {
    assert_eq!(std::fs::read(a.join("project/readme.md")).unwrap(), std::fs::read(b.join("project/readme.md")).unwrap());
    assert_eq!(std::fs::read(a.join("project/src/data.bin")).unwrap(), std::fs::read(b.join("project/src/data.bin")).unwrap());
}

fn no_parts(dir: &std::path::Path) {
    for entry in walk(dir) {
        assert!(!entry.to_string_lossy().ends_with(".sshcraft-part"), "left behind: {}", entry.display());
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        }
        out.push(p);
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_and_downloads_a_folder_tree() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    tree(local.path());
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [local.path().join("project")], "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!((job["files_done"].as_u64(), job["files_total"].as_u64()), (Some(2), Some(2)));
    assert_eq!(job["bytes_done"], 300_010);
    same_tree(local.path(), f.root.path());
    no_parts(f.root.path());

    let back = tempfile::tempdir().unwrap();
    let down = run(&f, "sftp.download", json!({ "id": f.id, "remote": ["/project"], "local_dir": back.path() })).await.unwrap();
    let job = ended(&f, &down["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    same_tree(f.root.path(), back.path());
    no_parts(back.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_taken_name_is_skipped_overwritten_or_kept_beside() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("note.txt"), b"new").unwrap();
    let file = local.path().join("note.txt");
    std::fs::write(f.root.path().join("note.txt"), b"old").unwrap();

    let skip = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file], "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &skip["transfer"]).await;
    assert_eq!(job["skipped"], 1, "skip is the default");
    assert_eq!(std::fs::read(f.root.path().join("note.txt")).unwrap(), b"old");

    let keep = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file], "remote_dir": "/", "conflict": "rename" })).await.unwrap();
    ended(&f, &keep["transfer"]).await;
    assert_eq!(std::fs::read(f.root.path().join("note (2).txt")).unwrap(), b"new");
    assert_eq!(std::fs::read(f.root.path().join("note.txt")).unwrap(), b"old");

    let over = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file], "remote_dir": "/", "conflict": "overwrite" })).await.unwrap();
    ended(&f, &over["transfer"]).await;
    assert_eq!(std::fs::read(f.root.path().join("note.txt")).unwrap(), b"new");

    let down =
        run(&f, "sftp.download", json!({ "id": f.id, "remote": ["/note (2).txt"], "local_dir": local.path(), "conflict": "rename" })).await.unwrap();
    ended(&f, &down["transfer"]).await;
    assert_eq!(std::fs::read(local.path().join("note (2).txt")).unwrap(), b"new");
    no_parts(f.root.path());
    no_parts(local.path());
}

/// A cancelled transfer leaves no part behind and is marked cancelled.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_upload_leaves_nothing_on_the_server() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    let big = local.path().join("big.bin");
    std::fs::write(&big, vec![7u8; 40 * 1024 * 1024]).unwrap();
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [big], "remote_dir": "/" })).await.unwrap();
    run(&f, "sftp.cancel", json!({ "transfer": up["transfer"] })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["state"]["kind"], "cancelled", "{job}");
    assert!(!f.root.path().join("big.bin").exists());
    no_parts(f.root.path());
    assert!(run(&f, "sftp.cancel", json!({ "transfer": up["transfer"] })).await.is_err(), "an ended job cannot be cancelled");
}

/// Names from the server that are not plain names here never become local paths; the rest of
/// the folder still comes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn hostile_names_from_the_server_are_refused_on_download() {
    let f = fixture().await;
    std::fs::create_dir(f.root.path().join("box")).unwrap();
    std::fs::write(f.root.path().join("box/good"), b"ok").unwrap();
    std::fs::write(f.root.path().join(r"box/back\slash"), b"bad").unwrap();
    std::fs::write(f.root.path().join("box/esc\u{1b}[2J"), b"bad").unwrap();
    let local = tempfile::tempdir().unwrap();
    let down = run(&f, "sftp.download", json!({ "id": f.id, "remote": ["/box"], "local_dir": local.path() })).await.unwrap();
    let job = ended(&f, &down["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(job["files_done"], 1);
    assert_eq!(job["errors"].as_array().unwrap().len(), 2, "{job}");
    let names: Vec<String> =
        std::fs::read_dir(local.path().join("box")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["good"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn transfers_need_absolute_local_paths() {
    let f = fixture().await;
    assert!(run(&f, "sftp.upload", json!({ "id": f.id, "local": ["rel.txt"], "remote_dir": "/" })).await.is_err());
    assert!(run(&f, "sftp.download", json!({ "id": f.id, "remote": ["/x"], "local_dir": "rel" })).await.is_err());
    assert!(run(&f, "sftp.upload", json!({ "id": f.id, "local": [], "remote_dir": "/" })).await.is_err());
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 241) as u8).collect()
}

/// A part a lost connection left goes on where it stopped: only the rest is sent.
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_goes_on_from_the_part_a_lost_connection_left() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    let data = pattern(300_000);
    std::fs::write(local.path().join("big.bin"), &data).unwrap();
    std::fs::write(f.root.path().join(".big.bin.sshcraft-part"), &data[..100_000]).unwrap();
    f.session.sftp().transfers().mark_resumable("r:/.big.bin.sshcraft-part".into());
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [local.path().join("big.bin")], "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(job["resumed"], 100_000);
    assert_eq!(job["bytes_done"], 300_000);
    assert_eq!(std::fs::read(f.root.path().join("big.bin")).unwrap(), data);
    no_parts(f.root.path());
}

/// A part this run did not leave (an older try's, or anyone's) is never continued: the file is
/// sent whole, and what was in the part is gone.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_this_run_did_not_leave_is_never_continued() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    let data = pattern(200_000);
    std::fs::write(local.path().join("f.bin"), &data).unwrap();
    std::fs::write(f.root.path().join(".f.bin.sshcraft-part"), vec![0xEE; 120_000]).unwrap();
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [local.path().join("f.bin")], "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["resumed"], 0, "{job}");
    assert_eq!(std::fs::read(f.root.path().join("f.bin")).unwrap(), data);

    // Marked, but longer than the file: not the same file, so it starts over too.
    std::fs::remove_file(f.root.path().join("f.bin")).unwrap();
    std::fs::write(f.root.path().join(".f.bin.sshcraft-part"), vec![0xEE; 250_000]).unwrap();
    f.session.sftp().transfers().mark_resumable("r:/.f.bin.sshcraft-part".into());
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [local.path().join("f.bin")], "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["resumed"], 0, "{job}");
    assert_eq!(std::fs::read(f.root.path().join("f.bin")).unwrap(), data);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_goes_on_from_its_part() {
    let f = fixture().await;
    let data = pattern(250_000);
    std::fs::write(f.root.path().join("log.txt"), &data).unwrap();
    let local = tempfile::tempdir().unwrap();
    let part = local.path().join(".log.txt.sshcraft-part");
    std::fs::write(&part, &data[..70_000]).unwrap();
    f.session.sftp().transfers().mark_resumable(format!("l:{}", part.display()));
    let down = run(&f, "sftp.download", json!({ "id": f.id, "remote": ["/log.txt"], "local_dir": local.path() })).await.unwrap();
    let job = ended(&f, &down["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(job["resumed"], 70_000);
    assert_eq!(std::fs::read(local.path().join("log.txt")).unwrap(), data);
    no_parts(local.path());
}

/// A retry sends only what did not get there: a file the first try finished is not sent again.
#[tokio::test(flavor = "multi_thread")]
async fn a_retry_leaves_the_files_that_got_there() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("a"), pattern(1_000)).unwrap();
    std::fs::write(local.path().join("b"), pattern(2_000)).unwrap();
    std::fs::create_dir_all(f.root.path().join("t/b")).unwrap();
    // "b" cannot replace a folder of that name: the first try ends with one file failed.
    let up = run(
        &f,
        "sftp.upload",
        json!({ "id": f.id, "local": [local.path().join("a"), local.path().join("b")], "remote_dir": "/t", "conflict": "overwrite" }),
    )
    .await
    .unwrap();
    let first = ended(&f, &up["transfer"]).await;
    assert_eq!((first["files_done"].as_u64(), first["errors"].as_array().map(Vec::len)), (Some(1), Some(1)), "{first}");
    std::fs::remove_dir(f.root.path().join("t/b")).unwrap();
    let again = run(&f, "sftp.retry", json!({ "transfer": up["transfer"] })).await.unwrap();
    let second = ended(&f, &again["transfer"]).await;
    assert_eq!(second["state"]["kind"], "done", "{second}");
    assert_eq!((second["files_total"].as_u64(), second["bytes_done"].as_u64()), (Some(1), Some(2_000)), "only b goes: {second}");
    assert_eq!(std::fs::read(f.root.path().join("t/b")).unwrap(), pattern(2_000));
    let done = run(&f, "sftp.retry", json!({ "transfer": again["transfer"] })).await.unwrap_err();
    assert_eq!(code(done), ErrorCode::NotSupported, "a finished job is not tried again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_connects_again_and_keeps_its_id() {
    let f = fixture().await;
    std::fs::write(f.root.path().join("x"), b"1").unwrap();
    let again = run(&f, "sftp.reconnect", json!({ "id": f.id })).await.unwrap();
    assert_eq!(again["home"], "/");
    let listing = run(&f, "sftp.list", json!({ "id": f.id, "path": "/" })).await.unwrap();
    assert_eq!(names(&listing), ["x"]);
    assert!(run(&f, "sftp.reconnect", json!({ "id": 999 })).await.is_err());
}

/// Android's picker hands over a file whose name is a number: the upload takes the name given,
/// checked like any other.
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_can_go_under_a_name_of_its_own() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("42"), b"the report").unwrap();
    let file = local.path().join("42");
    let up = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file], "remote_dir": "/", "name": "Bericht März.pdf" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(std::fs::read(f.root.path().join("Bericht März.pdf")).unwrap(), b"the report");
    assert!(!f.root.path().join("42").exists());
    for bad in ["../out", "a/b", ".."] {
        let err = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file], "remote_dir": "/", "name": bad })).await.unwrap_err();
        assert_eq!(code(err), ErrorCode::NameInvalid, "{bad}");
    }
    let two = run(&f, "sftp.upload", json!({ "id": f.id, "local": [file, file], "remote_dir": "/", "name": "x" })).await.unwrap_err();
    assert_eq!(code(two), ErrorCode::NameInvalid, "a name goes with one file only");
}

/// A download into a file the picker made overwrites it from the start, so nothing of what was
/// in it before stays at its end.
#[tokio::test(flavor = "multi_thread")]
async fn a_download_into_a_given_file_overwrites_it() {
    let f = fixture().await;
    let data = pattern(150_000);
    std::fs::write(f.root.path().join("remote.bin"), &data).unwrap();
    std::fs::create_dir(f.root.path().join("folder")).unwrap();
    let local = tempfile::tempdir().unwrap();
    let target = local.path().join("17");
    std::fs::write(&target, vec![0xAA; 400_000]).unwrap();
    let down = run(&f, "sftp.download_file", json!({ "id": f.id, "remote": "/remote.bin", "local_file": target })).await.unwrap();
    let job = ended(&f, &down["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(std::fs::read(&target).unwrap(), data);
    no_parts(local.path());

    let folder = run(&f, "sftp.download_file", json!({ "id": f.id, "remote": "/folder", "local_file": target })).await.unwrap();
    let job = ended(&f, &folder["transfer"]).await;
    assert_eq!(job["state"]["kind"], "failed", "a folder is not a file: {job}");
    assert_eq!(std::fs::read(&target).unwrap(), data, "a failure before anything was read leaves the file alone");
    assert!(run(&f, "sftp.download_file", json!({ "id": f.id, "remote": "/remote.bin", "local_file": "relative" })).await.is_err());
}
