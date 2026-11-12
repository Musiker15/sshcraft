mod common;

use axum::http::{Method, StatusCode};
use common::{Person, TestEnv};
use serde_json::json;
use sshcraft_proto::devicelog::DeviceLog;

#[tokio::test]
async fn register_needs_the_api_key() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    let body = json!({
        "account": a.public(), "name": "alice", "email": "alice@example.org",
        "log": DeviceLog { entries: vec![a.log.entries[0].clone()] },
    });
    let (status, err) = env
        .request(Method::POST, "/v1/accounts", &[], Some(body.clone()))
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("bad_api_key"))
    );
    let (status, err) = env
        .request(
            Method::POST,
            "/v1/accounts",
            &[("x-api-key", "wrong")],
            Some(body),
        )
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("bad_api_key"))
    );
    let (status, ok) = env.register(&a).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ok["account"], json!(a.public()));
}

#[tokio::test]
async fn register_validates_input() {
    let env = TestEnv::new().await;
    let key = env.api_key().await;
    let a = Person::new("alice");
    let b = Person::new("bob");
    let one = |p: &Person| DeviceLog {
        entries: vec![p.log.entries[0].clone()],
    };
    let mut two = Person::new("two");
    two.add_device();
    let mut long_device = Person::new("long");
    long_device.devices.clear();
    long_device.log.entries.clear();
    let keys = sshcraft_proto::identity::DeviceKeys::generate();
    let cert = sshcraft_proto::identity::DeviceCertificate::issue(
        &long_device.key,
        uuid::Uuid::new_v4(),
        keys.public(),
        &"x".repeat(101),
        "linux",
        1,
    );
    long_device.log.append_add(&long_device.key, cert, 1);

    let cases = vec![
        json!({"account": a.public(), "name": "  ", "email": "a@example.org", "log": one(&a)}),
        json!({"account": a.public(), "name": "n".repeat(101), "email": "a@example.org", "log": one(&a)}),
        json!({"account": a.public(), "name": "alice", "email": "alice.example.org", "log": one(&a)}),
        json!({"account": a.public(), "name": "alice", "email": "a@b@example.org", "log": one(&a)}),
        json!({"account": two.public(), "name": "two", "email": "two@example.org", "log": two.log}),
        json!({"account": a.public(), "name": "alice", "email": "a@example.org", "log": one(&b)}),
        json!({"account": long_device.public(), "name": "l", "email": "l@example.org", "log": long_device.log}),
    ];
    for (i, body) in cases.into_iter().enumerate() {
        let (status, err) = env
            .request(
                Method::POST,
                "/v1/accounts",
                &[("x-api-key", &key)],
                Some(body),
            )
            .await;
        assert_eq!(
            (status, err["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid")),
            "case {i}"
        );
    }
    env.register_ok(&a).await;
    let mut c = Person::new("carol");
    c.email = " ALICE@Example.org ".into();
    let (status, err) = env.register(&c).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::CONFLICT, Some("email_taken"))
    );
}

#[tokio::test]
async fn login_with_device_key() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let (status, me) = env
        .authed(&token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["name"], "alice");
    assert_eq!(me["email"], "alice@example.org");
    assert_eq!(me["account"], json!(a.public()));
}

#[tokio::test]
async fn first_login_needs_the_api_key_later_ones_do_not() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let (status, err) = env.login(&a, 0, None).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("bad_api_key"))
    );
    let key = env.api_key().await;
    let (status, _) = env.login(&a, 0, Some(&key)).await;
    assert_eq!(status, StatusCode::OK);
    env.store().rotate_api_key().await.unwrap();
    let (status, body) = env.login(&a, 0, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let b = Person::new("bob");
    let (status, _) = env
        .request(
            Method::POST,
            "/v1/accounts",
            &[("x-api-key", &key)],
            Some(
                json!({"account": b.public(), "name": "bob", "email": "bob@example.org",
                        "log": DeviceLog { entries: vec![b.log.entries[0].clone()] }}),
            ),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the old key no longer registers"
    );
}

#[tokio::test]
async fn challenge_is_single_use_and_expires() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let key = env.api_key().await;
    let (_, ch) = env
        .request(Method::POST, "/v1/auth/challenge", &[], None)
        .await;
    let challenge = ch["challenge"].as_str().unwrap().to_owned();
    let bytes = data_encoding::BASE64URL_NOPAD
        .decode(challenge.as_bytes())
        .unwrap();
    let dev = &a.devices[0];
    let sig =
        data_encoding::BASE64URL_NOPAD.encode(&dev.keys.sign_auth(&bytes, dev.id, common::ORIGIN));
    let body = json!({"device_id": dev.id, "challenge": challenge, "signature": sig});
    let (status, _) = env
        .request(
            Method::POST,
            "/v1/auth/login",
            &[("x-api-key", &key)],
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = env
        .request(
            Method::POST,
            "/v1/auth/login",
            &[("x-api-key", &key)],
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a challenge works once");

    let (_, ch) = env
        .request(Method::POST, "/v1/auth/challenge", &[], None)
        .await;
    let challenge = ch["challenge"].as_str().unwrap().to_owned();
    let bytes = data_encoding::BASE64URL_NOPAD
        .decode(challenge.as_bytes())
        .unwrap();
    let sig =
        data_encoding::BASE64URL_NOPAD.encode(&dev.keys.sign_auth(&bytes, dev.id, common::ORIGIN));
    env.advance(61);
    let (status, _) = env
        .request(
            Method::POST,
            "/v1/auth/login",
            &[("x-api-key", &key)],
            Some(json!({"device_id": dev.id, "challenge": challenge, "signature": sig})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a challenge expires after 60 s"
    );
}

#[tokio::test]
async fn signature_for_another_origin_is_refused() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let key = env.api_key().await;
    let (status, err) = env
        .login_for(&a, 0, Some(&key), "https://other.example.org")
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized"))
    );
}

#[tokio::test]
async fn expired_or_unknown_token_is_refused() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    let (status, _) = env
        .authed("AAAA", Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = env.request(Method::GET, "/v1/accounts/me", &[], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    env.advance(30 * 86_400 - 1);
    let (status, _) = env
        .authed(&token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    env.advance(2);
    let (status, err) = env
        .authed(&token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized"))
    );
}

#[tokio::test]
async fn locked_account_cannot_log_in_and_loses_its_session() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let token = env.token(&a, 0).await;
    env.store().set_locked(&a.public(), true).await.unwrap();
    let (status, err) = env.login(&a, 0, None).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("account_locked"))
    );
    let (status, _) = env
        .authed(&token, Method::GET, "/v1/accounts/me", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn blocked_device_cannot_log_in() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    env.token(&a, 0).await;
    env.store()
        .set_admin_blocked(a.devices[0].id, true)
        .await
        .unwrap();
    let (status, err) = env.login(&a, 0, None).await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("device_blocked"))
    );
}

#[tokio::test]
async fn challenge_flood_does_not_block_logins() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let now = env.state.now();
    for _ in 0..20_000 {
        env.state.challenges.issue(now).unwrap();
    }
    let key = env.api_key().await;
    let (status, body) = env.login(&a, 0, Some(&key)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn forged_challenge_is_refused() {
    let env = TestEnv::new().await;
    let a = Person::new("alice");
    env.register_ok(&a).await;
    let key = env.api_key().await;
    let (_, ch) = env
        .request(Method::POST, "/v1/auth/challenge", &[], None)
        .await;
    let mut bytes = data_encoding::BASE64URL_NOPAD
        .decode(ch["challenge"].as_str().unwrap().as_bytes())
        .unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let dev = &a.devices[0];
    let sig = dev.keys.sign_auth(&bytes, dev.id, common::ORIGIN);
    let body = json!({
        "device_id": dev.id,
        "challenge": data_encoding::BASE64URL_NOPAD.encode(&bytes),
        "signature": data_encoding::BASE64URL_NOPAD.encode(&sig),
    });
    let (status, _) = env
        .request(
            Method::POST,
            "/v1/auth/login",
            &[("x-api-key", &key)],
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oversized_body_is_too_large() {
    let env = TestEnv::new().await;
    let key = env.api_key().await;
    let big = json!({ "x": "a".repeat(2 << 20) });
    let (status, err) = env
        .request(
            Method::POST,
            "/v1/accounts",
            &[("x-api-key", &key)],
            Some(big),
        )
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("too_large"))
    );
    let broken = json!({"account": "not base64 !!", "name": "x", "email": "x@example.org", "log": {"entries": []}});
    let (status, err) = env
        .request(
            Method::POST,
            "/v1/accounts",
            &[("x-api-key", &key)],
            Some(broken),
        )
        .await;
    assert_eq!(
        (status, err["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
}
