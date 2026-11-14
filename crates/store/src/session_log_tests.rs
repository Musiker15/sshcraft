use super::*;
use crate::vault::MemoryMasterKey;

fn setup() -> (Store, Vault) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (store, vault)
}

fn new_meta(started_at: i64) -> LogMeta {
    LogMeta {
        id: LogId(Uuid::new_v4()),
        title: "web-1".into(),
        kind: LogKind::Ssh,
        host_id: Some(HostId(Uuid::new_v4())),
        started_at,
        ended_at: None,
        size: 0,
        truncated: false,
        marks: Vec::new(),
    }
}

fn mark(kind: MarkKind, offset: u64, text: &str) -> LogMark {
    LogMark {
        id: Uuid::new_v4(),
        offset,
        kind,
        text: text.into(),
        created_at: 1,
    }
}

/// A finished log of the given parts.
fn recorded(store: &Store, vault: &Vault, started_at: i64, parts: &[&[u8]]) -> LogMeta {
    let meta = new_meta(started_at);
    create(vault, store, &meta).unwrap();
    let mut size = 0;
    for (seq, part) in parts.iter().enumerate() {
        append_chunk(vault, store, meta.id, seq as u32, part, started_at + 10).unwrap();
        size += part.len() as u64;
    }
    finish(vault, store, meta.id, started_at + 100, size, false).unwrap()
}

#[test]
fn parts_read_back_in_order() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1000, &[b"one ", b"two ", b"three"]);
    assert_eq!(read(&vault, &store, meta.id).unwrap(), b"one two three");
    let listed = list(&vault, &store).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].meta, meta);
    assert_eq!(listed[0].location, None);
}

#[test]
fn a_missing_part_is_damage() {
    let (store, vault) = setup();
    let meta = new_meta(1);
    create(&vault, &store, &meta).unwrap();
    append_chunk(&vault, &store, meta.id, 0, b"a", 2).unwrap();
    append_chunk(&vault, &store, meta.id, 2, b"c", 2).unwrap();
    assert!(matches!(
        read(&vault, &store, meta.id),
        Err(LogError::Damaged)
    ));
}

#[test]
fn a_part_moved_to_another_position_does_not_open() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"first", b"second"]);
    // Swap the sealed blocks of both parts, as someone with access to the database could.
    store
        .conn
        .execute_batch(&format!(
            "UPDATE session_log_chunks SET seq = 99 WHERE log_id = '{id}' AND seq = 0;
             UPDATE session_log_chunks SET seq = 0 WHERE log_id = '{id}' AND seq = 1;
             UPDATE session_log_chunks SET seq = 1 WHERE log_id = '{id}' AND seq = 99;",
            id = meta.id.0
        ))
        .unwrap();
    assert!(matches!(
        read(&vault, &store, meta.id),
        Err(LogError::Damaged)
    ));
}

#[test]
fn a_part_of_another_log_does_not_open() {
    let (store, vault) = setup();
    let a = recorded(&store, &vault, 1, &[b"secret"]);
    let b = recorded(&store, &vault, 2, &[b"public"]);
    store
        .conn
        .execute(
            "UPDATE session_log_chunks SET sealed =
                 (SELECT sealed FROM session_log_chunks WHERE log_id = ?1)
             WHERE log_id = ?2",
            params![a.id.0.to_string(), b.id.0.to_string()],
        )
        .unwrap();
    assert!(matches!(read(&vault, &store, b.id), Err(LogError::Damaged)));
}

#[test]
fn a_finished_log_shorter_than_its_size_is_damaged() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"abc", b"def"]);
    store
        .conn
        .execute(
            "DELETE FROM session_log_chunks WHERE log_id = ?1 AND seq = 1",
            [meta.id.0.to_string()],
        )
        .unwrap();
    assert!(matches!(
        read(&vault, &store, meta.id),
        Err(LogError::Damaged)
    ));
}

#[test]
fn parts_without_header_are_not_listed() {
    let (store, vault) = setup();
    put_chunk(
        &vault,
        &store,
        Uuid::new_v4(),
        LogId(Uuid::new_v4()),
        0,
        b"early",
        1,
        None,
    )
    .unwrap();
    assert!(list(&vault, &store).unwrap().is_empty());
}

#[test]
fn chunk_size_is_limited() {
    let (store, vault) = setup();
    let meta = new_meta(1);
    create(&vault, &store, &meta).unwrap();
    let big = vec![b'x'; CHUNK_SIZE + 1];
    assert!(matches!(
        append_chunk(&vault, &store, meta.id, 0, &big, 1),
        Err(LogError::Invalid("chunk"))
    ));
    append_chunk(&vault, &store, meta.id, 0, &big[..CHUNK_SIZE], 1).unwrap();
}

#[test]
fn marks_are_checked() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"0123456789"]);
    let ok = vec![
        mark(MarkKind::Bookmark, 0, ""),
        mark(MarkKind::Bookmark, 10, "end"),
        mark(MarkKind::Comment, 5, "line one\nline two"),
    ];
    let saved = save_marks(&vault, &store, meta.id, ok.clone()).unwrap();
    assert_eq!(saved.marks, ok);
    assert_eq!(meta_of(&store, &vault, meta.id).marks, ok);

    for bad in [
        vec![mark(MarkKind::Bookmark, 11, "past the end")],
        vec![mark(MarkKind::Bookmark, 0, &"x".repeat(MAX_BOOKMARK + 1))],
        vec![mark(MarkKind::Bookmark, 0, "two\nlines")],
        vec![mark(MarkKind::Comment, 0, "  ")],
        vec![mark(MarkKind::Comment, 0, &"x".repeat(MAX_COMMENT + 1))],
        vec![mark(MarkKind::Comment, 0, "bell\u{7}")],
        (0..=MAX_MARKS)
            .map(|_| mark(MarkKind::Bookmark, 0, ""))
            .collect(),
    ] {
        assert!(
            matches!(
                save_marks(&vault, &store, meta.id, bad.clone()),
                Err(LogError::Invalid(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn marks_of_a_running_log_may_point_into_stored_parts() {
    let (store, vault) = setup();
    let meta = new_meta(1);
    create(&vault, &store, &meta).unwrap();
    append_chunk(&vault, &store, meta.id, 0, b"abcdef", 2).unwrap();
    save_marks(
        &vault,
        &store,
        meta.id,
        vec![mark(MarkKind::Bookmark, 6, "now")],
    )
    .unwrap();
    // Finishing keeps them.
    let done = finish(&vault, &store, meta.id, 3, 6, false).unwrap();
    assert_eq!(done.marks.len(), 1);
}

fn meta_of(store: &Store, vault: &Vault, id: LogId) -> LogMeta {
    meta(vault, store, id).unwrap().unwrap()
}

#[test]
fn title_is_checked() {
    let (store, vault) = setup();
    let mut meta = new_meta(1);
    meta.title = " ".into();
    assert!(matches!(
        create(&vault, &store, &meta),
        Err(LogError::Invalid("title"))
    ));
    meta.title = "x".repeat(MAX_TITLE + 1);
    assert!(matches!(
        create(&vault, &store, &meta),
        Err(LogError::Invalid("title"))
    ));
}

#[test]
fn delete_removes_header_and_parts() {
    let (store, vault) = setup();
    let gone = recorded(&store, &vault, 1, &[b"a", b"b"]);
    let kept = recorded(&store, &vault, 2, &[b"c"]);
    delete(&store, gone.id).unwrap();
    assert_eq!(chunk_ids(&store, gone.id).unwrap(), Vec::<String>::new());
    let listed: Vec<LogId> = list(&vault, &store)
        .unwrap()
        .into_iter()
        .map(|s| s.meta.id)
        .collect();
    assert_eq!(listed, vec![kept.id]);
    assert_eq!(read(&vault, &store, kept.id).unwrap(), b"c");
}

#[test]
fn expired_takes_only_ended_and_old_logs() {
    let (store, vault) = setup();
    let old = recorded(&store, &vault, 1_000, &[b"a"]); // ends at 1_100
    let new = recorded(&store, &vault, 5_000, &[b"b"]); // ends at 5_100
    let running = new_meta(0);
    create(&vault, &store, &running).unwrap();
    assert_eq!(expired(&store, 2_000).unwrap(), vec![old.id]);
    let mut both = expired(&store, 10_000).unwrap();
    both.sort_by_key(|id| id.0);
    let mut want = vec![old.id, new.id];
    want.sort_by_key(|id| id.0);
    assert_eq!(both, want);
}

#[test]
fn abandoned_logs_end_at_their_last_part() {
    let (store, vault) = setup();
    let with_parts = new_meta(100);
    create(&vault, &store, &with_parts).unwrap();
    append_chunk(&vault, &store, with_parts.id, 0, b"abc", 150).unwrap();
    append_chunk(&vault, &store, with_parts.id, 1, b"de", 180).unwrap();
    let empty = new_meta(300);
    create(&vault, &store, &empty).unwrap();
    let done = recorded(&store, &vault, 1, &[b"x"]);

    let mut ended = finish_abandoned(&vault, &store).unwrap();
    ended.sort_by_key(|id| id.0);
    let mut want = vec![with_parts.id, empty.id];
    want.sort_by_key(|id| id.0);
    assert_eq!(ended, want);

    let a = meta_of(&store, &vault, with_parts.id);
    assert_eq!((a.ended_at, a.size), (Some(180), 5));
    assert_eq!(read(&vault, &store, with_parts.id).unwrap(), b"abcde");
    let b = meta_of(&store, &vault, empty.id);
    assert_eq!((b.ended_at, b.size), (Some(300), 0));
    assert_eq!(meta_of(&store, &vault, done.id).ended_at, Some(101));
}

#[test]
fn search_finds_words_across_parts() {
    let (store, vault) = setup();
    let older = recorded(&store, &vault, 1, &[b"nothing here\r\n"]);
    let log = recorded(
        &store,
        &vault,
        2,
        &[
            b"$ systemctl status ngi",
            b"nx\r\n\x1b[32mActive\x1b[0m: running\r\n",
        ],
    );
    let hits = search(&vault, &store, "NGINX").unwrap();
    assert_eq!(
        hits,
        vec![LogHit {
            log_id: log.id,
            line: 0,
            snippet: "$ systemctl status nginx".into()
        }]
    );
    let hits = search(&vault, &store, "active: run").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 1);
    assert_eq!(search(&vault, &store, "here").unwrap()[0].log_id, older.id);
    assert!(search(&vault, &store, "n").unwrap().is_empty());
}

#[test]
fn search_stops_at_the_limit() {
    let (store, vault) = setup();
    let many = "match\n".repeat(MAX_HITS + 50);
    recorded(&store, &vault, 1, &[many.as_bytes()]);
    assert_eq!(search(&vault, &store, "match").unwrap().len(), MAX_HITS);
}

#[test]
fn long_lines_give_short_snippets() {
    let (store, vault) = setup();
    let line = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
    recorded(&store, &vault, 1, &[line.as_bytes()]);
    let hit = &search(&vault, &store, "needle").unwrap()[0];
    assert_eq!(hit.snippet.chars().count(), MAX_SNIPPET);
    assert!(hit.snippet.contains("needle"));
}

#[test]
fn location_moves_with_parts() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"a", b"b"]);
    let server = ServerId(Uuid::new_v4());
    set_location(&store, meta.id, Some(server)).unwrap();
    assert_eq!(location(&store, meta.id).unwrap(), Some(Some(server)));
    let places: Vec<Option<String>> = store
        .conn
        .prepare("SELECT location FROM session_log_chunks")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(places, vec![Some(server.0.to_string()); 2]);
    // Finishing again or saving marks keeps the place.
    save_marks(&vault, &store, meta.id, Vec::new()).unwrap();
    assert_eq!(list(&vault, &store).unwrap()[0].location, Some(server));
}

#[test]
fn session_log_setting_inherits_from_groups() {
    use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host};
    let parent = Group {
        id: GroupId(Uuid::new_v4()),
        name: "p".into(),
        parent_id: None,
        defaults: ConnectionSettings {
            session_log: Some(true),
            ..Default::default()
        },
        location: None,
        tags: Vec::new(),
    };
    let child = Group {
        id: GroupId(Uuid::new_v4()),
        name: "c".into(),
        parent_id: Some(parent.id),
        defaults: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
    };
    let mut host = Host {
        id: HostId(Uuid::new_v4()),
        name: "h".into(),
        group_id: Some(child.id),
        settings: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    let groups = vec![parent, child];
    assert_eq!(
        sshcraft_model::model::session_log_for(&host, &groups),
        Some(true)
    );
    host.settings.session_log = Some(false);
    assert_eq!(
        sshcraft_model::model::session_log_for(&host, &groups),
        Some(false)
    );
    host.group_id = None;
    host.settings.session_log = None;
    assert_eq!(sshcraft_model::model::session_log_for(&host, &groups), None);
}

#[test]
fn adding_a_mark_keeps_marks_added_meanwhile() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"0123456789"]);
    // The terminal sets a bookmark while a viewer still shows no marks.
    add_mark(
        &vault,
        &store,
        meta.id,
        mark(MarkKind::Bookmark, 4, "terminal"),
    )
    .unwrap();
    let from_viewer = mark(MarkKind::Comment, 8, "viewer");
    let saved = add_mark(&vault, &store, meta.id, from_viewer.clone()).unwrap();
    let texts: Vec<&str> = saved.marks.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["terminal", "viewer"]);
    let after = remove_mark(&vault, &store, meta.id, from_viewer.id).unwrap();
    assert_eq!(after.marks.len(), 1);
    assert_eq!(after.marks[0].text, "terminal");
}

#[test]
fn orphaned_local_parts_are_purged_synced_ones_kept() {
    let (store, vault) = setup();
    let kept = recorded(&store, &vault, 1, &[b"keep"]);
    let gone = LogId(Uuid::new_v4());
    append_chunk(&vault, &store, gone, 0, b"left over", 2).unwrap();
    let early = LogId(Uuid::new_v4());
    put_chunk(
        &vault,
        &store,
        Uuid::new_v4(),
        early,
        0,
        b"header follows",
        2,
        Some(ServerId(Uuid::new_v4())),
    )
    .unwrap();
    assert_eq!(purge_orphans(&store).unwrap(), 1);
    assert!(chunk_ids(&store, gone).unwrap().is_empty());
    assert_eq!(chunk_ids(&store, early).unwrap().len(), 1);
    assert_eq!(read(&vault, &store, kept.id).unwrap(), b"keep");
}

#[test]
fn running_tells_logs_being_recorded() {
    let (store, vault) = setup();
    let open = new_meta(1);
    create(&vault, &store, &open).unwrap();
    let done = recorded(&store, &vault, 2, &[b"x"]);
    assert!(running(&store, open.id).unwrap());
    assert!(!running(&store, done.id).unwrap());
    assert!(!running(&store, LogId(Uuid::new_v4())).unwrap());
}

#[test]
fn atomic_undoes_everything_on_error_and_nests() {
    let (store, vault) = setup();
    let meta = recorded(&store, &vault, 1, &[b"a", b"b"]);
    let server = ServerId(Uuid::new_v4());
    let result: Result<(), LogError> = store.atomic(|| {
        set_location(&store, meta.id, Some(server))?;
        delete(&store, meta.id)?;
        Err(LogError::Invalid("stop"))
    });
    assert!(result.is_err());
    assert_eq!(location(&store, meta.id).unwrap(), Some(None));
    assert_eq!(read(&vault, &store, meta.id).unwrap(), b"ab");
}

#[test]
fn search_bytes_counts_towards_one_limit() {
    let id = LogId(Uuid::new_v4());
    let mut hits = Vec::new();
    let text = "x match\n".repeat(150);
    search_bytes(id, text.as_bytes(), "match", &mut hits);
    search_bytes(id, text.as_bytes(), "match", &mut hits);
    assert_eq!(hits.len(), MAX_HITS);
    let mut none = Vec::new();
    search_bytes(id, b"match", "m", &mut none);
    assert!(none.is_empty());
}

#[test]
fn a_file_store_writes_ahead() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("s.db")).unwrap();
    let mode: String = store
        .conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}
