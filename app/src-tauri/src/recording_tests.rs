use super::*;
use sshcraft_core::model::HostId;
use sshcraft_core::session_log::LogKind;
use sshcraft_core::vault::MemoryMasterKey;

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
    Limits {
        chunk,
        max,
        flush_after: Duration::from_secs(5),
    }
}

fn read(store: &Mutex<Store>, vault: &Vault, id: LogId) -> Vec<u8> {
    session_log::read(vault, &store.lock().unwrap(), id).unwrap()
}

fn stored_meta(store: &Mutex<Store>, vault: &Vault, id: LogId) -> LogMeta {
    session_log::meta(vault, &store.lock().unwrap(), id)
        .unwrap()
        .unwrap()
}

fn parts(store: &Mutex<Store>, id: LogId) -> usize {
    session_log::chunk_ids(&store.lock().unwrap(), id)
        .unwrap()
        .len()
}

#[tokio::test]
async fn output_is_stored_in_parts() {
    let (store, vault) = setup();
    let recorder =
        Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default()).unwrap();
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
    let recorder =
        Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default()).unwrap();
    let id = recorder.id();
    let inner = Arc::new(Collect::default());
    let sink = LogSink {
        inner: inner.clone(),
        recorder,
    };
    sink.data(b"hello ".to_vec());
    sink.data(b"world".to_vec());
    sink.status(SessionStatus::Ended {
        reason: crate::sessions::EndReason::Exited,
        code: None,
        detail: None,
    });
    assert_eq!(*inner.data.lock().unwrap(), b"hello world");
    assert_eq!(inner.statuses.lock().unwrap().len(), 1);
    assert_eq!(stored_meta(&store, &vault, id).size, 11);
    assert!(stored_meta(&store, &vault, id).ended_at.is_some());
    assert_eq!(read(&store, &vault, id), b"hello world");
}

#[tokio::test]
async fn dropping_the_recorder_ends_the_log() {
    let (store, vault) = setup();
    let recorder =
        Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default()).unwrap();
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
    let recorder = Recorder::start(store.clone(), vault, &meta(), Limits::default()).unwrap();
    recorder.record(b"prompt$ ");
    tokio::time::sleep(Duration::from_millis(4_500)).await;
    assert_eq!(parts(&store, recorder.id()), 0);
    tokio::time::sleep(Duration::from_millis(1_600)).await;
    assert_eq!(parts(&store, recorder.id()), 1);
}

#[tokio::test]
async fn recording_stops_at_the_limit() {
    let (store, vault) = setup();
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), small(16, 40)).unwrap();
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
    let recorder = Recorder::start(store.clone(), vault.clone(), &meta(), small(4, 1000)).unwrap();
    let id = recorder.id();
    let inner = Arc::new(Collect::default());
    let sink = LogSink {
        inner: inner.clone(),
        recorder,
    };
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
    let recorder =
        Recorder::start(store.clone(), vault.clone(), &meta(), Limits::default()).unwrap();
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
    assert!(
        housekeeping(&store, &vault, 0, now, false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        housekeeping(&store, &vault, 30, now, false).unwrap(),
        vec![ids[0]]
    );
    let left: Vec<LogId> = session_log::list(&vault, &store.lock().unwrap())
        .unwrap()
        .into_iter()
        .map(|s| s.meta.id)
        .collect();
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
