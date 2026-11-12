mod common;

use axum::http::{Method, StatusCode};
use common::{Auth, Person, TestEnv, account_path};
use serde_json::json;
use sshcraft_proto::identity::{DeviceCertificate, DeviceKeys};
use uuid::Uuid;

#[tokio::test]
async fn new_device_enrolls_with_api_key_and_logs_in() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let second = env.enroll_new_device(&mut a).await;
    let token = env.token(&a, second).await;
    let (status, list) = env
        .authed(&token, Method::GET, "/v1/accounts/me/devices", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let devices = list["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert!(
        devices
            .iter()
            .all(|d| d["active"] == true && d["admin_blocked"] == false)
    );
    assert_eq!(devices[1]["device_id"], json!(a.devices[second].id));
}

#[tokio::test]
async fn append_without_session_or_key_is_refused() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let (_, entry) = a.add_device();
    let (status, _) = env.append(&a, &[entry], Auth::None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn foreign_session_cannot_append() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    let b = Person::new("bob");
    env.register_ok(&a).await;
    env.register_ok(&b).await;
    let token_b = env.token(&b, 0).await;
    let (_, entry) = a.add_device();
    let (status, err) = env.append(&a, &[entry], Auth::Token(&token_b)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden"))
    );
}

#[tokio::test]
async fn forged_or_stale_entries_are_refused() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;

    let (_, mut forged) = a.add_device();
    forged.signature[0] ^= 1;
    let (status, err) = env.append(&a, &[forged], Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    a.log.entries.pop();
    a.devices.pop();

    let (_, entry) = a.add_device();
    let (status, _) = env
        .append(&a, std::slice::from_ref(&entry), Auth::Token(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, err) = env.append(&a, &[entry], Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str(), err["current"].as_u64()),
        (StatusCode::CONFLICT, Some("conflict"), Some(2))
    );

    let keys = DeviceKeys::generate();
    let cert = DeviceCertificate::issue(&a.key, Uuid::new_v4(), keys.public(), "x", "linux", 1);
    let signer = &a.devices[0];
    a.log
        .append_add_signed_by_device_for_test(signer.id, &signer.keys, cert, 1);
    let bad = a.log.entries.last().cloned().unwrap();
    let (status, err) = env.append(&a, &[bad], Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
}

#[tokio::test]
async fn revoke_ends_the_session_of_the_revoked_device() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let second = env.enroll_new_device(&mut a).await;
    let first_token = env.token(&a, 0).await;
    let second_token = env.token(&a, second).await;
    let revoke = a.revoke_by_device(0, second);
    let (status, body) = env.append(&a, &[revoke], Auth::Token(&first_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["len"], 3);
    let (status, _) = env
        .authed(&second_token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert!(matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ));
    let (status, err) = env.login(&a, second, None).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("device_inactive"))
    );
    let (status, _) = env
        .authed(&first_token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn device_id_of_another_account_is_refused() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    let mut b = Person::new("bob");
    env.register_ok(&a).await;
    env.register_ok(&b).await;
    let token_b = env.token(&b, 0).await;
    let keys = DeviceKeys::generate();
    let cert = DeviceCertificate::issue(&b.key, a.devices[0].id, keys.public(), "x", "linux", 1);
    b.log.append_add(&b.key, cert, 1);
    let entry = b.log.entries.last().cloned().unwrap();
    let (status, err) = env.append(&b, &[entry], Auth::Token(&token_b)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::CONFLICT, Some("device_id_taken"))
    );
}

#[tokio::test]
async fn log_and_lookup_are_readable_for_other_accounts() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    let b = Person::new("bob");
    env.register_ok(&a).await;
    env.register_ok(&b).await;
    let token_b = env.token(&b, 0).await;
    let path = format!("/v1/accounts/{}/log", account_path(&a.public()));
    let (status, log) = env.authed(&token_b, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(log["entries"], json!(a.log.entries));
    let (status, log) = env
        .authed(&token_b, Method::GET, &format!("{path}?from=1"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(log["entries"], json!([]));
    let (status, found) = env
        .authed(
            &token_b,
            Method::GET,
            "/v1/accounts/lookup?email=%20Alice@Example.ORG%20",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (found["account"].clone(), found["name"].as_str()),
        (json!(a.public()), Some("alice"))
    );
    let (status, _) = env
        .authed(
            &token_b,
            Method::GET,
            "/v1/accounts/lookup?email=nobody@example.org",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, info) = env
        .authed(
            &token_b,
            Method::GET,
            &format!("/v1/accounts/{}", account_path(&a.public())),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["name"], "alice");
    assert!(info.get("email").is_none());
    let unknown = Person::new("ghost");
    let (status, _) = env
        .authed(
            &token_b,
            Method::GET,
            &format!("/v1/accounts/{}/log", account_path(&unknown.public())),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, err) = env
        .authed(&token_b, Method::GET, "/v1/accounts/not-base64!/log", None)
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
}

#[tokio::test]
async fn too_many_entries_are_refused() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let entries: Vec<_> = (0..101).map(|_| a.add_device().1).collect();
    let (status, err) = env.append(&a, &entries, Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    let (status, err) = env.append(&a, &[], Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
}

#[tokio::test]
async fn api_key_append_accepts_only_account_signed_entries() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let second = env.enroll_new_device(&mut a).await;
    env.store()
        .set_admin_blocked(a.devices[second].id, true)
        .await
        .unwrap();
    // The blocked device still holds its key and the shared API key.
    let revoke = a.revoke_by_device(second, 0);
    let (status, err) = env.append(&a, &[revoke], Auth::ApiKey).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden"))
    );
    let first = env.store().device(a.devices[0].id).await.unwrap().unwrap();
    assert!(first.active);
}

#[tokio::test]
async fn log_is_capped() {
    let env = TestEnv::new().await;
    let mut a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    // Fill the log to 999 entries straight in the store; the server would verify each append.
    let entries: Vec<_> = (0..998).map(|_| a.add_device().1).collect();
    let changes = sshcraft_server::store::DeviceChanges {
        added: a.devices[1..].iter().map(|d| d.cert.clone()).collect(),
        revoked: vec![],
    };
    env.store()
        .append_log(&a.public(), 1, &entries, &changes)
        .await
        .unwrap();
    let (_, last) = a.add_device();
    let (status, body) = env.append(&a, &[last], Auth::Token(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, one_more) = a.add_device();
    let (status, err) = env.append(&a, &[one_more], Auth::Token(&token)).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
}
