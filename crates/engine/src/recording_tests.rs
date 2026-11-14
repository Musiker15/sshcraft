use super::*;
use sshcraft_model::model::HostId;
use sshcraft_store::session_log::LogKind;
use sshcraft_store::vault::MemoryMasterKey;

#[derive(Default)]
struct Collect {
    data: Mutex<Vec<u8>>,
    statuses: Mutex<Vec<SessionStatus>>,
}

impl SessionSink for Collect {
    fn data(&self, bytes: Vec<u8>) {
        self.data.lock().unwrap().extend(bytes);
    }
    fn status(&self, status: SessionStatus) {
        self.statuses.lock().unwrap().push(status);
    }
}

fn setup() -> (Arc<Mutex<Store>>, Arc<Vault>) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (Arc::new(Mutex::new(store)), Arc::new(vault))
}

fn meta() -> LogMeta {
    LogMeta {
        id: LogId(Uuid::new_v4()),
        title: "web".into(),
        kind: LogKind::Ssh,
        host_id: Some(HostId(Uuid::new_v4())),
        started_at: now_ms(),
        ended_at: None,
        size: 0,
        truncated: false,
        marks: Vec::new(),
    }
}

fn small(chunk: usize, max: u64) -> Limits {
    Limits { chunk, max, flush_after: Duration::from_secs(5) }
}

fn read(store: &Mutex<Store>, vault: &Vault, id: LogId) -> Vec<u8> {
    session_log::read(vault, &store.lock().unwrap(), id).unwrap()
}

fn stored_meta(store: &Mutex<Store>, vault: &Vault, id: LogId) -> LogMeta {
    session_log::meta(vault, &store.lock().unwrap(), id).unwrap().unwrap()
}

fn parts(store: &Mutex<Store>, id: LogId) -> usize {
    session_log::chunk_ids(&store.lock().unwrap(), id).unwrap().len()
}

#[tokio::test]
async fn output_is_stored_in_parts() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    let output: Vec<u8> = (0..70 * 1024).map(|i| (i % 251) as u8).collect();
    for piece in output.chunks(1000) {
        recorder.record(piece);
    }
    assert_eq!(parts(&store, recorder.id()), 2);
    let done = recorder.finish().unwrap();
    assert_eq!(parts(&store, recorder.id()), 3);
    assert_eq!(done.size, output.len() as u64);
    assert!(done.ended_at.is_some());
    assert_eq!(read(&store, &vault, recorder.id()), output);
}

#[tokio::test]
async fn the_sink_passes_everything_on_and_ends_the_log() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    let id = recorder.id();
    let inner = Arc::new(Collect::default());
    let sink = LogSink { inner: inner.clone(), recorder };
    sink.data(b"hello ".to_vec());
    sink.data(b"world".to_vec());
    sink.status(SessionStatus::Ended { reason: crate::sessions::EndReason::Exited, code: None, detail: None });
    assert_eq!(*inner.data.lock().unwrap(), b"hello world");
    assert_eq!(inner.statuses.lock().unwrap().len(), 1);
    assert_eq!(stored_meta(&store, &vault, id).size, 11);
    assert!(stored_meta(&store, &vault, id).ended_at.is_some());
    assert_eq!(read(&store, &vault, id), b"hello world");
}

#[tokio::test]
async fn dropping_the_recorder_ends_the_log() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    let id = recorder.id();
    recorder.record(b"closed tab");
    drop(recorder);
    let meta = stored_meta(&store, &vault, id);
    assert!(meta.ended_at.is_some());
    assert_eq!(read(&store, &vault, id), b"closed tab");
}

#[tokio::test(start_paused = true)]
async fn waiting_output_is_stored_after_five_seconds() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault, &meta(), Limits::default(), None).unwrap();
    recorder.record(b"prompt$ ");
    tokio::time::sleep(Duration::from_millis(4_500)).await;
    assert_eq!(parts(&store, recorder.id()), 0);
    tokio::time::sleep(Duration::from_millis(1_600)).await;
    assert_eq!(parts(&store, recorder.id()), 1);
}

#[tokio::test]
async fn recording_stops_at_the_limit() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), small(16, 40), None).unwrap();
    recorder.record(&[b'a'; 30]);
    recorder.record(&[b'b'; 30]);
    recorder.record(&[b'c'; 30]);
    let done = recorder.finish().unwrap();
    assert!(done.truncated);
    assert_eq!(done.size, 40);
    let mut want = vec![b'a'; 30];
    want.extend([b'b'; 10]);
    assert_eq!(read(&store, &vault, recorder.id()), want);
}

#[tokio::test]
async fn a_failing_store_stops_only_the_recording() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), small(4, 1000), None).unwrap();
    let id = recorder.id();
    let inner = Arc::new(Collect::default());
    let sink = LogSink { inner: inner.clone(), recorder };
    sink.data(b"abcd".to_vec());
    // Storing parts fails from now on.
    rusqlite_break(&store);
    sink.data(b"efgh".to_vec());
    sink.data(b"ijkl".to_vec());
    assert_eq!(*inner.data.lock().unwrap(), b"abcdefghijkl");
    drop(sink);
    let meta = stored_meta(&store, &vault, id);
    assert_eq!(meta.size, 4);
}

/// Makes every further part fail to store, as a full disk would.
fn rusqlite_break(store: &Mutex<Store>) {
    store
        .lock()
        .unwrap()
        .execute_raw(
            "CREATE TRIGGER no_parts BEFORE INSERT ON session_log_chunks
             BEGIN SELECT RAISE(FAIL, 'disk full'); END;",
        )
        .unwrap();
}

#[tokio::test]
async fn a_bookmark_points_at_the_output_so_far() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    recorder.record(b"0123456789");
    let mark = recorder.bookmark("deploy").unwrap();
    assert_eq!(mark.offset, 10);
    assert_eq!(mark.kind, MarkKind::Bookmark);
    // The waiting output was stored with it.
    assert_eq!(parts(&store, recorder.id()), 1);
    recorder.record(b"more");
    let done = recorder.finish().unwrap();
    assert_eq!(done.marks, vec![mark]);
}

#[tokio::test]
async fn housekeeping_deletes_only_expired_logs() {
    let (store, vault) = setup();
    let day = 24 * 60 * 60 * 1000;
    let now = 100 * day;
    let mut ids = Vec::new();
    for ended in [now - 40 * day, now - 10 * day] {
        let meta = meta();
        let guard = store.lock().unwrap();
        session_log::create(&vault, &guard, &meta).unwrap();
        session_log::finish(&vault, &guard, meta.id, ended, 0, false).unwrap();
        ids.push(meta.id);
    }
    assert!(housekeeping(&store, &vault, 0, now, false).unwrap().is_empty());
    // Both logs are on this device only: nothing to send anywhere.
    assert!(housekeeping(&store, &vault, 30, now, false).unwrap().is_empty());
    let left: Vec<LogId> = session_log::list(&vault, &store.lock().unwrap()).unwrap().into_iter().map(|s| s.meta.id).collect();
    assert_eq!(left, vec![ids[1]]);
}

#[tokio::test]
async fn housekeeping_at_start_ends_abandoned_logs() {
    let (store, vault) = setup();
    let meta = meta();
    session_log::create(&vault, &store.lock().unwrap(), &meta).unwrap();
    housekeeping(&store, &vault, 0, now_ms(), false).unwrap();
    assert!(stored_meta(&store, &vault, meta.id).ended_at.is_none());
    housekeeping(&store, &vault, 0, now_ms(), true).unwrap();
    assert!(stored_meta(&store, &vault, meta.id).ended_at.is_some());
}

#[tokio::test]
async fn the_end_is_reported_once() {
    let (store, vault) = setup();
    let ended = Arc::new(Mutex::new(Vec::new()));
    let seen = ended.clone();
    let report: OnFinish = Arc::new(move |id| seen.lock().unwrap().push(id));
    let recorder = Recorder::start(store, vault, &meta(), Limits::default(), Some(report)).unwrap();
    recorder.record(b"x");
    recorder.finish();
    let id = recorder.id();
    drop(recorder);
    assert_eq!(*ended.lock().unwrap(), vec![id]);
}

fn server_row(store: &Store) -> sshcraft_model::model::ServerId {
    let server = sshcraft_model::model::ServerId(Uuid::new_v4());
    store
        .add_server(&sshcraft_store::store::ServerRow {
            id: server,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [7; 32],
            device_id: Uuid::new_v4(),
            sealed_device: Vec::new(),
            sync_enabled: true,
            sync_keys: false,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    server
}

#[tokio::test]
async fn expired_synced_logs_are_deleted_at_their_server_too() {
    let (store, vault) = setup();
    let day = 24 * 60 * 60 * 1000;
    let now = 100 * day;
    let meta = meta();
    let server = {
        let guard = store.lock().unwrap();
        session_log::create(&vault, &guard, &meta).unwrap();
        session_log::append_chunk(&vault, &guard, meta.id, 0, b"old", 1).unwrap();
        session_log::finish(&vault, &guard, meta.id, now - 40 * day, 3, false).unwrap();
        let server = server_row(&guard);
        session_log::set_location(&guard, meta.id, Some(server)).unwrap();
        server
    };
    assert_eq!(housekeeping(&store, &vault, 30, now, false).unwrap(), vec![server]);
    let outbox = store.lock().unwrap().outbox(server).unwrap();
    assert!(outbox.contains(&(sshcraft_store::store::ItemKind::Log, meta.id.0.to_string())));
    assert_eq!(outbox.len(), 2);
}

#[tokio::test]
async fn a_session_without_output_leaves_no_log() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    assert!(recorder.finish().is_none());
    assert!(session_log::list(&vault, &store.lock().unwrap()).unwrap().is_empty());
}

#[tokio::test]
async fn no_bookmark_after_the_end() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default(), None).unwrap();
    recorder.record(b"output");
    recorder.finish();
    assert!(matches!(recorder.bookmark("late"), Err(LogError::NotFound)));
    let id = recorder.id();
    assert!(stored_meta(&store, &vault, id).marks.is_empty());
}

#[tokio::test]
async fn a_cut_recording_is_no_longer_active() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store, vault, &meta(), small(16, 20), None).unwrap();
    assert!(recorder.is_active());
    recorder.record(&[b'x'; 30]);
    assert!(!recorder.is_active());
}
