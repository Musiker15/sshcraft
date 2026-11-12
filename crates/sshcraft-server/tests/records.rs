mod common;

use axum::http::{Method, StatusCode};
use common::{Person, TestEnv};
use serde_json::{Value, json};
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use uuid::Uuid;

/// Owner Alice, writer Wendy, reader Rita, stranger Sam, one shared area.
struct World {
    env: TestEnv,
    owner: Person,
    writer: Person,
    reader: Person,
    tokens: [String; 4],
    area: AreaId,
    key: AreaKey,
}

impl World {
    async fn new() -> Self {
        let env = TestEnv::new().await;
        let (owner, writer, reader, stranger) = (
            Person::new("alice"),
            Person::new("wendy"),
            Person::new("rita"),
            Person::new("sam"),
        );
        let mut tokens = Vec::new();
        for p in [&owner, &writer, &reader, &stranger] {
            env.register_ok(p).await;
            tokens.push(env.token(p, 0).await);
        }
        let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
        let manifest = AreaManifest::sign(
            area,
            1,
            1,
            &key,
            owner.public(),
            vec![
                Member {
                    account: writer.public(),
                    role: Role::Write,
                },
                Member {
                    account: reader.public(),
                    role: Role::Read,
                },
            ],
            owner.devices[0].id,
            &owner.devices[0].keys,
        );
        let (status, body) = env
            .authed(
                &tokens[0],
                Method::PUT,
                &format!("/v1/areas/{}/manifest", area.0),
                Some(json!(manifest)),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        Self {
            env,
            owner,
            writer,
            reader,
            tokens: tokens.try_into().unwrap(),
            area,
            key,
        }
    }

    fn seal(&self, by: &Person, record_id: Uuid, version: u64, body: &[u8]) -> SealedRecord {
        let header = RecordHeader {
            record_id,
            area_id: self.area,
            version,
            key_version: 1,
            author_account: by.public(),
            author_device: by.devices[0].id,
        };
        SealedRecord::seal(header, body, &self.key, &by.devices[0].keys)
    }

    async fn put(&self, token: usize, record: &SealedRecord) -> (StatusCode, Value) {
        let path = format!(
            "/v1/areas/{}/records/{}",
            self.area.0, record.header.record_id
        );
        self.env
            .authed(&self.tokens[token], Method::PUT, &path, Some(json!(record)))
            .await
    }

    async fn get(&self, token: usize, query: &str) -> (StatusCode, Value) {
        let path = format!("/v1/areas/{}/records{query}", self.area.0);
        self.env
            .authed(&self.tokens[token], Method::GET, &path, None)
            .await
    }
}

const OWNER: usize = 0;
const WRITER: usize = 1;
const READER: usize = 2;
const STRANGER: usize = 3;

fn code(r: &(StatusCode, Value)) -> (StatusCode, Option<&str>) {
    (r.0, r.1["code"].as_str())
}

#[tokio::test]
async fn owner_writes_and_member_reads() {
    let w = World::new().await;
    let id = Uuid::new_v4();
    let (status, body) = w.put(OWNER, &w.seal(&w.owner, id, 1, b"one")).await;
    assert_eq!((status, body["seq"].as_u64()), (StatusCode::OK, Some(1)));
    let other = Uuid::new_v4();
    let (status, body) = w.put(OWNER, &w.seal(&w.owner, other, 1, b"two")).await;
    assert_eq!((status, body["seq"].as_u64()), (StatusCode::OK, Some(2)));
    let (status, all) = w.get(READER, "?since=0").await;
    assert_eq!(status, StatusCode::OK);
    let seqs: Vec<u64> = all["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, vec![1, 2]);
    assert_eq!(all["more"], false);
    let (_, later) = w.get(READER, "?since=1").await;
    assert_eq!(later["records"].as_array().unwrap().len(), 1);
    assert_eq!(
        later["records"][0]["record"]["header"]["record_id"],
        json!(other)
    );
}

#[tokio::test]
async fn write_role_is_enforced() {
    let w = World::new().await;
    let r = w
        .put(READER, &w.seal(&w.reader, Uuid::new_v4(), 1, b"x"))
        .await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, Some("forbidden")));
    let r = w
        .put(WRITER, &w.seal(&w.writer, Uuid::new_v4(), 1, b"x"))
        .await;
    assert_eq!(r.0, StatusCode::OK, "{}", r.1);
    let r = w.get(STRANGER, "").await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, Some("forbidden")));
}

#[tokio::test]
async fn author_must_be_the_session() {
    let mut w = World::new().await;
    let r = w
        .put(WRITER, &w.seal(&w.owner, Uuid::new_v4(), 1, b"x"))
        .await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, Some("forbidden")));
    // Another device of the owner, but the session belongs to device 0.
    let second = w.env.enroll_new_device(&mut w.owner).await;
    let header = RecordHeader {
        record_id: Uuid::new_v4(),
        area_id: w.area,
        version: 1,
        key_version: 1,
        author_account: w.owner.public(),
        author_device: w.owner.devices[second].id,
    };
    let record = SealedRecord::seal(header, b"x", &w.key, &w.owner.devices[second].keys);
    let r = w.put(OWNER, &record).await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, Some("forbidden")));
}

#[tokio::test]
async fn wrong_key_version_or_path_is_invalid() {
    let w = World::new().await;
    let mut record = w.seal(&w.owner, Uuid::new_v4(), 1, b"x");
    record.header.key_version = 2;
    assert_eq!(
        code(&w.put(OWNER, &record).await),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    let record = w.seal(&w.owner, Uuid::new_v4(), 1, b"x");
    let path = format!("/v1/areas/{}/records/{}", w.area.0, Uuid::new_v4());
    let r = w
        .env
        .authed(&w.tokens[OWNER], Method::PUT, &path, Some(json!(record)))
        .await;
    assert_eq!(code(&r), (StatusCode::BAD_REQUEST, Some("invalid")));
    let path = format!(
        "/v1/areas/{}/records/{}",
        Uuid::new_v4(),
        record.header.record_id
    );
    let r = w
        .env
        .authed(&w.tokens[OWNER], Method::PUT, &path, Some(json!(record)))
        .await;
    assert_eq!(code(&r), (StatusCode::BAD_REQUEST, Some("invalid")));
}

#[tokio::test]
async fn concurrent_writers_one_wins() {
    let w = World::new().await;
    let id = Uuid::new_v4();
    assert_eq!(
        w.put(OWNER, &w.seal(&w.owner, id, 1, b"one")).await.0,
        StatusCode::OK
    );
    let (by_owner, by_writer) = (
        w.seal(&w.owner, id, 2, b"a"),
        w.seal(&w.writer, id, 2, b"b"),
    );
    let (r1, r2) = tokio::join!(w.put(OWNER, &by_owner), w.put(WRITER, &by_writer));
    let statuses = [r1.0, r2.0];
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    let conflict = if r1.0 == StatusCode::CONFLICT {
        r1.1
    } else {
        r2.1
    };
    assert_eq!(conflict["current"], 2);
}

#[tokio::test]
async fn locked_owner_freezes_the_area() {
    let w = World::new().await;
    w.env
        .store()
        .set_locked(&w.owner.public(), true)
        .await
        .unwrap();
    let r = w
        .put(WRITER, &w.seal(&w.writer, Uuid::new_v4(), 1, b"x"))
        .await;
    assert_eq!(code(&r), (StatusCode::FORBIDDEN, Some("account_locked")));
    assert_eq!(w.get(READER, "").await.0, StatusCode::OK);
}

#[tokio::test]
async fn oversized_ciphertext_is_refused() {
    let w = World::new().await;
    let big = vec![7u8; 65 * 1024];
    let r = w
        .put(OWNER, &w.seal(&w.owner, Uuid::new_v4(), 1, &big))
        .await;
    assert_eq!(code(&r), (StatusCode::PAYLOAD_TOO_LARGE, Some("too_large")));
}

#[tokio::test]
async fn paging_reports_more() {
    let w = World::new().await;
    for _ in 0..3 {
        assert_eq!(
            w.put(OWNER, &w.seal(&w.owner, Uuid::new_v4(), 1, b"x"))
                .await
                .0,
            StatusCode::OK
        );
    }
    let (status, page) = w.get(OWNER, "?since=0&limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["records"].as_array().unwrap().len(), 2);
    assert_eq!(page["more"], true);
    let (_, rest) = w.get(OWNER, "?since=2&limit=2").await;
    assert_eq!(rest["records"].as_array().unwrap().len(), 1);
    assert_eq!(rest["more"], false);
}
