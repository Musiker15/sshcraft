mod common;

use axum::http::{Method, StatusCode};
use common::{Auth, Person, TestEnv};
use futures::StreamExt;
use serde_json::{Value, json};
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Starts the router on a free local port; returns the `ws://` address of `/v1/events`.
async fn serve(env: &mut TestEnv) -> String {
    env.state.ws_recheck = Duration::from_millis(200);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = sshcraft_server::app(env.state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("ws://{addr}/v1/events")
}

async fn connect(
    url: &str,
    token: Option<&str>,
) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
    let mut request = url.into_client_request().unwrap();
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(ws, _)| ws)
}

/// The next event as JSON; `None` when the connection closed. Pings are skipped.
async fn next_event(ws: &mut Ws, wait: Duration) -> Option<Value> {
    tokio::time::timeout(wait, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => return Some(serde_json::from_str(&text).unwrap()),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await
    .expect("no event within the time limit")
}

/// Asserts that no event arrives within `wait`.
async fn silent(ws: &mut Ws, wait: Duration) {
    let got = tokio::time::timeout(wait, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => return text.to_string(),
                Some(Ok(_)) => continue,
                other => return format!("{other:?}"),
            }
        }
    })
    .await;
    assert!(got.is_err(), "unexpected: {got:?}");
}

const WAIT: Duration = Duration::from_secs(5);

async fn put_manifest(
    env: &TestEnv,
    token: &str,
    owner: &Person,
    area: AreaId,
    version: u64,
    key: &AreaKey,
    members: Vec<Member>,
) {
    let m = AreaManifest::sign(
        area,
        version,
        1,
        key,
        owner.public(),
        members,
        owner.devices[0].id,
        &owner.devices[0].keys,
    );
    let (status, body) = env
        .authed(
            token,
            Method::PUT,
            &format!("/v1/areas/{}/manifest", area.0),
            Some(json!(m)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn put_record(env: &TestEnv, token: &str, by: &Person, area: AreaId, key: &AreaKey) -> Uuid {
    let id = Uuid::new_v4();
    let header = RecordHeader {
        record_id: id,
        area_id: area,
        version: 1,
        key_version: 1,
        author_account: by.public(),
        author_device: by.devices[0].id,
    };
    let record = SealedRecord::seal(header, b"x", key, &by.devices[0].keys);
    let (status, body) = env
        .authed(
            token,
            Method::PUT,
            &format!("/v1/areas/{}/records/{id}", area.0),
            Some(json!(record)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

#[tokio::test]
async fn member_gets_record_events_stranger_does_not() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let (a, b, c) = (
        Person::new("alice"),
        Person::new("bob"),
        Person::new("carol"),
    );
    for p in [&a, &b, &c] {
        env.register_ok(p).await;
    }
    let (ta, tb, tc) = (
        env.token(&a, 0).await,
        env.token(&b, 0).await,
        env.token(&c, 0).await,
    );
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(
        &env,
        &ta,
        &a,
        area,
        1,
        &key,
        vec![Member {
            account: b.public(),
            role: Role::Read,
        }],
    )
    .await;
    let mut ws_b = connect(&url, Some(&tb)).await.unwrap();
    let mut ws_c = connect(&url, Some(&tc)).await.unwrap();
    let id = put_record(&env, &ta, &a, area, &key).await;
    let event = next_event(&mut ws_b, WAIT).await.expect("event");
    assert_eq!(
        event,
        json!({ "type": "record", "area_id": area.0, "record_id": id, "version": 1, "seq": 1 })
    );
    silent(&mut ws_c, Duration::from_millis(500)).await;
}

#[tokio::test]
async fn revoked_device_is_disconnected() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let second = env.enroll_new_device(&mut a).await;
    let (t0, t1) = (env.token(&a, 0).await, env.token(&a, second).await);
    let mut ws = connect(&url, Some(&t1)).await.unwrap();
    let revoke = a.revoke_by_device(0, second);
    assert_eq!(
        env.append(&a, &[revoke], Auth::Token(&t0)).await.0,
        StatusCode::OK
    );
    let mut ended = false;
    while let Some(event) = next_event(&mut ws, WAIT).await {
        if event["type"] == "session_ended" {
            ended = true;
            break;
        }
    }
    assert!(ended, "SessionEnded must arrive");
    assert!(
        next_event(&mut ws, WAIT).await.is_none(),
        "the connection closes"
    );
}

#[tokio::test]
async fn admin_block_disconnects_within_the_recheck() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let mut ws = connect(&url, Some(&token)).await.unwrap();
    env.store()
        .set_admin_blocked(a.devices[0].id, true)
        .await
        .unwrap();
    let event = next_event(&mut ws, WAIT).await.expect("event");
    assert_eq!(event, json!({ "type": "session_ended" }));
    assert!(next_event(&mut ws, WAIT).await.is_none());
}

#[tokio::test]
async fn upgrade_without_session_is_refused() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    match connect(&url, None).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED)
        }
        other => panic!("expected 401, got {:?}", other.map(|_| ())),
    }
    assert!(connect(&url, Some("AAAA")).await.is_err());
}

#[tokio::test]
async fn removed_member_hears_about_it() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let (a, b) = (Person::new("alice"), Person::new("bob"));
    env.register_ok(&a).await;
    env.register_ok(&b).await;
    let (ta, tb) = (env.token(&a, 0).await, env.token(&b, 0).await);
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(
        &env,
        &ta,
        &a,
        area,
        1,
        &key,
        vec![Member {
            account: b.public(),
            role: Role::Read,
        }],
    )
    .await;
    let mut ws = connect(&url, Some(&tb)).await.unwrap();
    put_manifest(&env, &ta, &a, area, 2, &key, vec![]).await;
    let event = next_event(&mut ws, WAIT).await.expect("event");
    assert_eq!(
        event,
        json!({ "type": "manifest", "area_id": area.0, "version": 2 })
    );
    put_record(&env, &ta, &a, area, &key).await;
    silent(&mut ws, Duration::from_millis(500)).await;
}

#[tokio::test]
async fn too_many_connections_per_device_are_refused() {
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let mut open = Vec::new();
    for _ in 0..sshcraft_server::hub::MAX_PER_DEVICE {
        open.push(connect(&url, Some(&token)).await.unwrap());
    }
    match connect(&url, Some(&token)).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE)
        }
        other => panic!("expected 503, got {:?}", other.map(|_| ())),
    }
    drop(open);
}

#[tokio::test]
async fn oversized_client_message_closes_the_connection() {
    use futures::SinkExt;
    let mut env = TestEnv::new().await;
    let url = serve(&mut env).await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let mut ws = connect(&url, Some(&token)).await.unwrap();
    ws.send(Message::Text("x".repeat(64 * 1024).into()))
        .await
        .unwrap();
    assert!(
        next_event(&mut ws, WAIT).await.is_none(),
        "the server closes the connection"
    );
}

#[tokio::test]
async fn events_without_upgrade_answer_in_the_error_format() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let (status, body) = env.authed(&token, Method::GET, "/v1/events", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid");
}
