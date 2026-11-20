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

/// The job of an upload under a name of its own is shown under that name, not the file's.
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_under_a_name_is_shown_under_it() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("121"), b"x").unwrap();
    let up =
        run(&f, "sftp.upload", json!({ "id": f.id, "local": [local.path().join("121")], "remote_dir": "/", "name": "Notiz.txt" })).await.unwrap();
    let job = ended(&f, &up["transfer"]).await;
    assert_eq!(job["label"], "Notiz.txt");
}

/// A server whose host key changed says so by its own code, not as a session failure with two
/// fingerprints nobody reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_changed_host_key_has_its_own_code() {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::open(SessionConfig::for_tests(dir.path())).unwrap().0;
    let (server, host) = crate::testing::trusted_test_host(&session).await;
    let root = tempfile::tempdir().unwrap();
    server.serve_sftp(root.path());
    session.store().remove_host_key("127.0.0.1", server.port, &server.host_fingerprint).unwrap();
    crate::testing::set_known_host(&session, "127.0.0.1", server.port, "SHA256:other");
    let err = session.execute(Caller::User, "sftp.open", json!({ "host_id": host })).await.unwrap_err();
    assert_eq!(code(err), ErrorCode::HostKeyChanged);
}

/// A second server beside the fixture's, with its own folder, open as an SFTP session.
async fn second(f: &Fixture) -> (sshcraft_ssh::testing::TestServer, tempfile::TempDir, u64) {
    let auth = sshcraft_ssh::testing::TestAuth { quiet: true, ..Default::default() };
    let (server, host) = crate::testing::test_host(&f.session, "second host", auth, true).await;
    let root = tempfile::tempdir().unwrap();
    server.serve_sftp(root.path());
    let opened = run(f, "sftp.open", json!({ "host_id": host })).await.unwrap();
    (server, root, opened["id"].as_u64().unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn copies_a_folder_tree_from_server_to_server() {
    let f = fixture().await;
    let (_server, root, to) = second(&f).await;
    tree(f.root.path());
    let copy = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/project"], "to": to, "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &copy["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!((job["direction"].as_str(), job["sftp"].as_u64(), job["source"].as_u64()), (Some("copy"), Some(to), Some(f.id)));
    assert_eq!((job["files_done"].as_u64(), job["files_total"].as_u64()), (Some(2), Some(2)));
    assert_eq!(job["bytes_done"], 300_010);
    same_tree(f.root.path(), root.path());
    no_parts(root.path());

    // The default leaves a taken name alone; "keep both" puts the copy beside it.
    std::fs::write(f.root.path().join("note.txt"), b"new").unwrap();
    std::fs::write(root.path().join("note.txt"), b"old").unwrap();
    let skip = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/note.txt"], "to": to, "remote_dir": "/" })).await.unwrap();
    assert_eq!(ended(&f, &skip["transfer"]).await["skipped"], 1);
    let keep =
        run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/note.txt"], "to": to, "remote_dir": "/", "conflict": "rename" })).await.unwrap();
    ended(&f, &keep["transfer"]).await;
    assert_eq!(std::fs::read(root.path().join("note.txt")).unwrap(), b"old");
    assert_eq!(std::fs::read(root.path().join("note (2).txt")).unwrap(), b"new");
}

/// A name from one server goes to the other only if it is a plain name; the rest still comes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn hostile_names_are_not_copied() {
    let f = fixture().await;
    let (_server, root, to) = second(&f).await;
    std::fs::create_dir(f.root.path().join("box")).unwrap();
    std::fs::write(f.root.path().join("box/good"), b"ok").unwrap();
    std::fs::write(f.root.path().join("box/esc\u{1b}[2J"), b"bad").unwrap();
    let copy = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/box"], "to": to, "remote_dir": "/" })).await.unwrap();
    let job = ended(&f, &copy["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(job["errors"].as_array().unwrap().len(), 1, "{job}");
    let names: Vec<String> =
        std::fs::read_dir(root.path().join("box")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["good"]);
}

/// Closing the session a copy reads from stops it, and the target keeps no part.
#[tokio::test(flavor = "multi_thread")]
async fn closing_the_source_stops_a_copy() {
    let f = fixture().await;
    let (_server, root, to) = second(&f).await;
    std::fs::write(f.root.path().join("big.bin"), vec![7u8; 40 * 1024 * 1024]).unwrap();
    let copy = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/big.bin"], "to": to, "remote_dir": "/" })).await.unwrap();
    run(&f, "sftp.close", json!({ "id": f.id })).await.unwrap();
    let job = ended(&f, &copy["transfer"]).await;
    assert_eq!(job["state"]["kind"], "cancelled", "{job}");
    assert!(!root.path().join("big.bin").exists());
    no_parts(root.path());
}

/// A cancelled copy can be tried again; it reads from its source session once more.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_copy_can_be_tried_again() {
    let f = fixture().await;
    let (_server, root, to) = second(&f).await;
    let data = pattern(40 * 1024 * 1024);
    std::fs::write(f.root.path().join("data.bin"), &data).unwrap();
    let copy = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/data.bin"], "to": to, "remote_dir": "/" })).await.unwrap();
    run(&f, "sftp.cancel", json!({ "transfer": copy["transfer"] })).await.unwrap();
    assert_eq!(ended(&f, &copy["transfer"]).await["state"]["kind"], "cancelled");
    let again = run(&f, "sftp.retry", json!({ "transfer": copy["transfer"] })).await.unwrap();
    let job = ended(&f, &again["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    assert_eq!(std::fs::read(root.path().join("data.bin")).unwrap(), data);
    no_parts(root.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copy_needs_both_sessions_and_some_paths() {
    let f = fixture().await;
    let err = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/x"], "to": 999, "remote_dir": "/" })).await.unwrap_err();
    assert_eq!(code(err), ErrorCode::NotFound);
    let err = run(&f, "sftp.copy", json!({ "from": 999, "remote": ["/x"], "to": f.id, "remote_dir": "/" })).await.unwrap_err();
    assert_eq!(code(err), ErrorCode::NotFound);
    assert!(run(&f, "sftp.copy", json!({ "from": f.id, "remote": [], "to": f.id, "remote_dir": "/" })).await.is_err());
}

/// Closing the source while a file is being written stops the copy too, and its part goes: the
/// reads that fail after the close are the cancel, not a lost connection to go on from.
#[tokio::test(flavor = "multi_thread")]
async fn closing_the_source_mid_file_leaves_no_part() {
    let f = fixture().await;
    let (_server, root, to) = second(&f).await;
    std::fs::write(f.root.path().join("big.bin"), vec![7u8; 64 * 1024 * 1024]).unwrap();
    let copy = run(&f, "sftp.copy", json!({ "from": f.id, "remote": ["/big.bin"], "to": to, "remote_dir": "/" })).await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let all = run(&f, "sftp.transfers", json!({})).await.unwrap();
        let job = all.as_array().unwrap().iter().find(|j| j["id"] == copy["transfer"]).cloned().unwrap();
        if job["bytes_done"].as_u64().unwrap_or(0) > 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline && job["state"]["kind"] != "done", "the copy did not start: {job}");
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    run(&f, "sftp.close", json!({ "id": f.id })).await.unwrap();
    let job = ended(&f, &copy["transfer"]).await;
    assert_eq!(job["state"]["kind"], "cancelled", "{job}");
    assert!(!root.path().join("big.bin").exists());
    no_parts(root.path());
}

fn set_mtime(path: &std::path::Path, secs: u64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)).unwrap();
}

fn mtime(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

async fn sync(f: &Fixture, local: &std::path::Path, remote: &str, direction: &str) -> Value {
    let started = run(f, "sftp.sync", json!({ "id": f.id, "local": local, "remote": remote, "direction": direction })).await.unwrap();
    let job = ended(f, &started["transfer"]).await;
    assert_eq!(job["state"]["kind"], "done", "{job}");
    job
}

/// Syncing a folder up sends what is missing or newer here into the folder there (not into a
/// folder of the same name), keeps the times, leaves what is newer there and deletes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn syncing_up_sends_only_newer_files_and_deletes_nothing() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    let site = local.path().join("site");
    std::fs::create_dir_all(site.join("css")).unwrap();
    std::fs::write(site.join("index.html"), b"v1").unwrap();
    std::fs::write(site.join("css/main.css"), b"body{}").unwrap();
    set_mtime(&site.join("index.html"), 1_700_000_000);
    set_mtime(&site.join("css/main.css"), 1_700_000_000);
    let www = f.root.path().join("www");
    std::fs::create_dir(&www).unwrap();
    std::fs::write(www.join("only-there.txt"), b"stays").unwrap();

    let job = sync(&f, &site, "/www", "upload").await;
    assert_eq!((job["files_done"].as_u64(), job["skipped"].as_u64()), (Some(2), Some(0)), "{job}");
    assert_eq!(std::fs::read(www.join("index.html")).unwrap(), b"v1");
    assert_eq!(std::fs::read(www.join("css/main.css")).unwrap(), b"body{}");
    assert!(!www.join("site").exists(), "the folder's contents go into the folder there");
    assert_eq!(mtime(&www.join("index.html")), 1_700_000_000, "the time goes with the file");

    let again = sync(&f, &site, "/www", "upload").await;
    assert_eq!((again["files_done"].as_u64(), again["skipped"].as_u64()), (Some(0), Some(2)), "nothing changed: {again}");

    std::fs::write(site.join("index.html"), b"v2").unwrap();
    set_mtime(&site.join("index.html"), 1_700_000_100);
    std::fs::write(www.join("css/main.css"), b"edited there").unwrap();
    set_mtime(&www.join("css/main.css"), 1_800_000_000);
    let third = sync(&f, &site, "/www", "upload").await;
    assert_eq!((third["files_done"].as_u64(), third["skipped"].as_u64()), (Some(1), Some(1)), "{third}");
    assert_eq!(std::fs::read(www.join("index.html")).unwrap(), b"v2");
    assert_eq!(std::fs::read(www.join("css/main.css")).unwrap(), b"edited there", "newer there stays");
    assert_eq!(std::fs::read(www.join("only-there.txt")).unwrap(), b"stays");
    no_parts(f.root.path());
}

/// Syncing down is the same the other way: newer files from there come here, with their time.
#[tokio::test(flavor = "multi_thread")]
async fn syncing_down_brings_newer_files_here() {
    let f = fixture().await;
    let www = f.root.path().join("www");
    std::fs::create_dir_all(www.join("logs")).unwrap();
    std::fs::write(www.join("app.conf"), b"port=1").unwrap();
    std::fs::write(www.join("logs/today.log"), b"line").unwrap();
    set_mtime(&www.join("app.conf"), 1_700_000_000);
    set_mtime(&www.join("logs/today.log"), 1_700_000_000);
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("mine.txt"), b"stays").unwrap();

    let job = sync(&f, local.path(), "/www", "download").await;
    assert_eq!((job["files_done"].as_u64(), job["skipped"].as_u64()), (Some(2), Some(0)), "{job}");
    assert_eq!(std::fs::read(local.path().join("logs/today.log")).unwrap(), b"line");
    assert!(!local.path().join("www").exists());
    assert_eq!(mtime(&local.path().join("app.conf")), 1_700_000_000);

    std::fs::write(local.path().join("app.conf"), b"port=2 (here)").unwrap();
    set_mtime(&local.path().join("app.conf"), 1_800_000_000);
    std::fs::write(www.join("logs/today.log"), b"line\nline").unwrap();
    set_mtime(&www.join("logs/today.log"), 1_700_000_100);
    let again = sync(&f, local.path(), "/www", "download").await;
    assert_eq!((again["files_done"].as_u64(), again["skipped"].as_u64()), (Some(1), Some(1)), "{again}");
    assert_eq!(std::fs::read(local.path().join("app.conf")).unwrap(), b"port=2 (here)");
    assert_eq!(std::fs::read(local.path().join("logs/today.log")).unwrap(), b"line\nline");
    assert_eq!(std::fs::read(local.path().join("mine.txt")).unwrap(), b"stays");
    no_parts(local.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sync_needs_two_folders() {
    let f = fixture().await;
    let local = tempfile::tempdir().unwrap();
    let file = local.path().join("a.txt");
    std::fs::write(&file, b"a").unwrap();
    std::fs::create_dir(f.root.path().join("www")).unwrap();
    let err = run(&f, "sftp.sync", json!({ "id": f.id, "local": file, "remote": "/www", "direction": "upload" })).await.unwrap_err();
    assert_eq!(code(err), ErrorCode::NotFound);
    assert!(run(&f, "sftp.sync", json!({ "id": f.id, "local": "rel", "remote": "/www", "direction": "upload" })).await.is_err());
    assert!(run(&f, "sftp.sync", json!({ "id": f.id, "local": local.path(), "remote": "/www", "direction": "copy" })).await.is_err());
    // A remote path that is no folder ends the job.
    let started = run(&f, "sftp.sync", json!({ "id": f.id, "local": local.path(), "remote": "/missing", "direction": "upload" })).await.unwrap();
    let job = ended(&f, &started["transfer"]).await;
    assert_eq!(job["state"]["kind"], "failed", "{job}");
    assert!(!f.root.path().join("missing").exists(), "nothing is made on the server");
}

/// What the stand-in editor was started with: the editor and the file.
type Started = std::sync::Arc<std::sync::Mutex<Vec<(std::path::PathBuf, std::path::PathBuf)>>>;

/// Names a text editor (this test program, which is a file) and records what it is started with
/// instead of starting anything.
async fn with_editor(f: &Fixture) -> (std::path::PathBuf, Started) {
    let editor = std::env::current_exe().unwrap();
    let mut settings = run(f, "settings.get", json!({})).await.unwrap();
    settings["sftp"]["editor"] = json!(editor);
    run(f, "settings.save", json!({ "settings": settings })).await.unwrap();
    let started: Started = std::sync::Arc::default();
    let seen = started.clone();
    f.session.set_editor_launcher(std::sync::Arc::new(move |editor: &std::path::Path, file: &std::path::Path| {
        seen.lock().unwrap().push((editor.to_path_buf(), file.to_path_buf()));
        Ok(())
    }));
    (editor, started)
}

/// Changes a file and its size, so a change is seen whatever the clock.
fn rewrite(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_opens_in_the_editor_and_each_save_goes_back() {
    let f = fixture().await;
    let (editor, started) = with_editor(&f).await;
    std::fs::write(f.root.path().join("notes.txt"), b"one").unwrap();
    let opened = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/notes.txt" })).await.unwrap();
    let local = std::path::PathBuf::from(opened["local"].as_str().unwrap());
    assert!(local.starts_with(f._dir.path().join("edits")), "{}", local.display());
    assert_eq!(std::fs::read(&local).unwrap(), b"one");
    assert_eq!(*started.lock().unwrap(), [(editor, local.clone())]);

    // Nothing saved yet: nothing goes.
    let report = run(&f, "sftp.edit_sync", json!({})).await.unwrap();
    assert_eq!(report["uploaded"], json!([]));

    rewrite(&local, "one and two");
    let report = run(&f, "sftp.edit_sync", json!({})).await.unwrap();
    assert_eq!(report["uploaded"], json!([opened["edit"]]), "{report}");
    assert_eq!(std::fs::read(f.root.path().join("notes.txt")).unwrap(), b"one and two");
    // The file kept its permissions, through its part.
    assert_eq!(f.server.sftp_mode("/.notes.txt.sshcraft-part").map(|m| m & 0o7777), Some(0o644));
    no_parts(f.root.path());

    // Once sent, the same save is not sent again, and a later one goes too.
    assert_eq!(run(&f, "sftp.edit_sync", json!({})).await.unwrap()["uploaded"], json!([]));
    rewrite(&local, "three");
    assert_eq!(run(&f, "sftp.edit_sync", json!({})).await.unwrap()["uploaded"], json!([opened["edit"]]));
    assert_eq!(std::fs::read(f.root.path().join("notes.txt")).unwrap(), b"three");

    // Opening it again starts the editor on the same copy.
    let again = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/notes.txt" })).await.unwrap();
    assert_eq!(again["edit"], opened["edit"]);
    assert_eq!(started.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_changed_on_the_server_is_not_overwritten() {
    let f = fixture().await;
    with_editor(&f).await;
    let remote = f.root.path().join("conf");
    std::fs::write(&remote, b"a").unwrap();
    let opened = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/conf" })).await.unwrap();
    let local = std::path::PathBuf::from(opened["local"].as_str().unwrap());
    std::fs::write(&remote, b"changed there").unwrap();
    rewrite(&local, "changed here");
    let report = run(&f, "sftp.edit_sync", json!({})).await.unwrap();
    assert_eq!(report["conflicts"], json!([opened["edit"]]), "{report}");
    assert_eq!(std::fs::read(&remote).unwrap(), b"changed there");
    // It is said once, not every time the copies are looked at.
    assert_eq!(run(&f, "sftp.edit_sync", json!({})).await.unwrap()["conflicts"], json!([]));
    let refused = run(&f, "sftp.edit_save", json!({ "edit": opened["edit"] })).await.unwrap_err();
    assert_eq!(code(refused), ErrorCode::ChangedElsewhere);
    run(&f, "sftp.edit_save", json!({ "edit": opened["edit"], "force": true })).await.unwrap();
    assert_eq!(std::fs::read(&remote).unwrap(), b"changed here");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_named_editor_opens_and_only_a_small_plain_file() {
    let f = fixture().await;
    std::fs::write(f.root.path().join("small"), b"x").unwrap();
    let none = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/small" })).await.unwrap_err();
    assert_eq!(code(none), ErrorCode::EditorMissing);

    // A path that is not a program file is not started.
    let mut settings = run(&f, "settings.get", json!({})).await.unwrap();
    settings["sftp"]["editor"] = json!(f.root.path().join("no-such-editor"));
    run(&f, "settings.save", json!({ "settings": settings.clone() })).await.unwrap();
    let missing = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/small" })).await.unwrap_err();
    assert_eq!(code(missing), ErrorCode::EditorMissing);
    // Nor is anything a shell would read: a relative name is dropped when the settings are saved.
    settings["sftp"]["editor"] = json!("notepad");
    let saved = run(&f, "settings.save", json!({ "settings": settings })).await.unwrap();
    assert_eq!(saved["sftp"]["editor"], Value::Null);

    let (_, started) = with_editor(&f).await;
    std::fs::create_dir(f.root.path().join("folder")).unwrap();
    let folder = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/folder" })).await.unwrap_err();
    assert_eq!(code(folder), ErrorCode::NotSupported);
    let big = std::fs::File::create(f.root.path().join("big")).unwrap();
    big.set_len(crate::sftp::edits::MAX_EDIT_BYTES + 1).unwrap();
    let large = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/big" })).await.unwrap_err();
    assert_eq!(code(large), ErrorCode::FileTooLarge);
    assert!(started.lock().unwrap().is_empty(), "nothing started for a refused file");
    assert_eq!(run(&f, "sftp.edits", json!({})).await.unwrap(), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_or_closing_deletes_the_copy() {
    let f = fixture().await;
    with_editor(&f).await;
    std::fs::write(f.root.path().join("a"), b"a").unwrap();
    let opened = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/a" })).await.unwrap();
    let local = std::path::PathBuf::from(opened["local"].as_str().unwrap());
    run(&f, "sftp.edit_stop", json!({ "edit": opened["edit"] })).await.unwrap();
    assert!(!local.exists());
    assert!(run(&f, "sftp.edit_stop", json!({ "edit": opened["edit"] })).await.is_err());

    let opened = run(&f, "sftp.edit", json!({ "id": f.id, "remote": "/a" })).await.unwrap();
    let local = std::path::PathBuf::from(opened["local"].as_str().unwrap());
    run(&f, "sftp.close", json!({ "id": f.id })).await.unwrap();
    assert!(!local.exists());
    assert_eq!(run(&f, "sftp.edits", json!({})).await.unwrap(), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn copies_a_crash_left_go_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("edits").join("7");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("secret.conf"), b"x").unwrap();
    let _session = Session::open(SessionConfig::for_tests(dir.path())).unwrap().0;
    assert!(!dir.path().join("edits").join("7").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_cannot_open_a_file_in_the_editor() {
    let f = fixture().await;
    with_editor(&f).await;
    std::fs::write(f.root.path().join("a"), b"a").unwrap();
    let agent = Caller::Agent { name: AgentName::new("claude-code").unwrap(), via: Via::Mcp, conn: ConnId(1) };
    for (id, params) in [
        ("sftp.edit", json!({ "id": f.id, "remote": "/a" })),
        ("sftp.edits", json!({})),
        ("sftp.edit_sync", json!({})),
        ("sftp.edit_save", json!({ "edit": 1, "force": true })),
        ("sftp.edit_stop", json!({ "edit": 1 })),
    ] {
        assert!(f.session.execute(agent.clone(), id, params).await.is_err(), "{id}");
    }
}
