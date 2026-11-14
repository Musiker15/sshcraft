mod common;

use axum::http::{Method, StatusCode};
use common::{Auth, Person, TestEnv};
use serde_json::{Value, json};
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role, seal_area_key};
use uuid::Uuid;

/// A manifest for `area` owned by `owner`, signed by the owner's device `device`.
fn manifest(owner: &Person, device: usize, area: AreaId, version: u64, key: &AreaKey, members: Vec<Member>) -> AreaManifest {
    AreaManifest::sign(area, version, 1, key, owner.public(), members, owner.devices[device].id, &owner.devices[device].keys)
}

fn member(p: &Person, role: Role) -> Member {
    Member { account: p.public(), role }
}

async fn put_manifest(env: &TestEnv, token: &str, m: &AreaManifest) -> (StatusCode, Value) {
    let path = format!("/v1/areas/{}/manifest", m.area_id.0);
    env.authed(token, Method::PUT, &path, Some(json!(m))).await
}

fn code(status: StatusCode, body: &Value) -> (StatusCode, Option<&str>) {
    (status, body["code"].as_str())
}

/// Alice (owner), Bob and Carol registered and logged in.
async fn three(env: &TestEnv) -> (Person, Person, Person, String, String, String) {
    let (a, b, c) = (Person::new("alice"), Person::new("bob"), Person::new("carol"));
    for p in [&a, &b, &c] {
        env.register_ok(p).await;
    }
    let (ta, tb, tc) = (env.token(&a, 0).await, env.token(&b, 0).await, env.token(&c, 0).await);
    (a, b, c, ta, tb, tc)
}

#[tokio::test]
async fn owner_creates_and_updates_an_area() {
    let env = TestEnv::new().await;
    let (a, _, _, ta, _, _) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    let (status, body) = put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v2 = manifest(&a, 0, area, 2, &key, vec![]);
    assert_eq!(put_manifest(&env, &ta, &v2).await.0, StatusCode::OK);
    let (status, list) = env.authed(&ta, Method::GET, "/v1/areas", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["areas"], json!([{ "owner": a.public(), "manifest": v2 }]));
}

#[tokio::test]
async fn version_must_follow() {
    let env = TestEnv::new().await;
    let (a, _, _, ta, _, _) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    let (status, body) = put_manifest(&env, &ta, &manifest(&a, 0, area, 2, &key, vec![])).await;
    assert_eq!((code(status, &body), body["current"].as_u64()), ((StatusCode::CONFLICT, Some("conflict")), Some(0)));
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![])).await;
    let (status, body) = put_manifest(&env, &ta, &manifest(&a, 0, area, 3, &key, vec![])).await;
    assert_eq!((code(status, &body), body["current"].as_u64()), ((StatusCode::CONFLICT, Some("conflict")), Some(1)));
}

#[tokio::test]
async fn manifest_by_someone_else_is_refused() {
    let env = TestEnv::new().await;
    let (mut a, b, _, ta, tb, _) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![])).await;
    // Bob claims Alice's area for himself.
    let (status, body) = put_manifest(&env, &tb, &manifest(&b, 0, area, 2, &key, vec![])).await;
    assert_eq!(code(status, &body), (StatusCode::FORBIDDEN, Some("forbidden")));
    // Bob sends a manifest that names Alice as owner but is signed by his device.
    let mut forged = manifest(&a, 0, area, 2, &key, vec![]);
    forged.author_device = b.devices[0].id;
    let (status, body) = put_manifest(&env, &tb, &forged).await;
    assert_eq!(code(status, &body), (StatusCode::FORBIDDEN, Some("forbidden")));
    // Alice's second device is revoked; a manifest signed by it is refused.
    let second = env.enroll_new_device(&mut a).await;
    let revoke = a.revoke_by_device(0, second);
    assert_eq!(env.append(&a, &[revoke], Auth::Token(&ta)).await.0, StatusCode::OK);
    let (status, body) = put_manifest(&env, &ta, &manifest(&a, second, area, 2, &key, vec![])).await;
    assert_eq!(code(status, &body), (StatusCode::BAD_REQUEST, Some("invalid")));
    // A manifest whose area id differs from the path is refused.
    let other = manifest(&a, 0, AreaId(Uuid::new_v4()), 1, &key, vec![]);
    let (status, body) = env.authed(&ta, Method::PUT, &format!("/v1/areas/{}/manifest", area.0), Some(json!(other))).await;
    assert_eq!(code(status, &body), (StatusCode::BAD_REQUEST, Some("invalid")));
}

#[tokio::test]
async fn members_must_exist() {
    let env = TestEnv::new().await;
    let (a, b, _, ta, _, _) = three(&env).await;
    let ghost = Person::new("ghost");
    let key = AreaKey::generate();
    for members in [vec![member(&ghost, Role::Read)], vec![member(&a, Role::Write)], vec![member(&b, Role::Read), member(&b, Role::Write)]] {
        let m = manifest(&a, 0, AreaId(Uuid::new_v4()), 1, &key, members);
        let (status, body) = put_manifest(&env, &ta, &m).await;
        assert_eq!(code(status, &body), (StatusCode::BAD_REQUEST, Some("invalid")));
    }
}

#[tokio::test]
async fn member_sees_the_area_stranger_does_not() {
    let env = TestEnv::new().await;
    let (a, b, _, ta, tb, tc) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![member(&b, Role::Read)])).await;
    let (_, list) = env.authed(&tb, Method::GET, "/v1/areas", None).await;
    assert_eq!(list["areas"].as_array().unwrap().len(), 1);
    let (_, list) = env.authed(&tc, Method::GET, "/v1/areas", None).await;
    assert_eq!(list["areas"], json!([]));
    let path = format!("/v1/areas/{}/envelope", area.0);
    let (status, body) = env.authed(&tc, Method::GET, &path, None).await;
    assert_eq!(code(status, &body), (StatusCode::FORBIDDEN, Some("forbidden")));
}

#[tokio::test]
async fn envelopes_go_to_the_right_device_only() {
    let env = TestEnv::new().await;
    let (a, b, c, ta, tb, tc) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![member(&b, Role::Read)])).await;
    let for_a = seal_area_key(&key, area, 1, &a.devices[0].cert).unwrap();
    let for_b = seal_area_key(&key, area, 1, &b.devices[0].cert).unwrap();
    let put = format!("/v1/areas/{}/envelopes", area.0);
    let get = format!("/v1/areas/{}/envelope", area.0);
    let (status, body) = env.authed(&ta, Method::PUT, &put, Some(json!({ "envelopes": [for_a, for_b] }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, mine) = env.authed(&tb, Method::GET, &get, None).await;
    assert_eq!((status, mine), (StatusCode::OK, json!(for_b)));
    assert_eq!(env.authed(&tc, Method::GET, &get, None).await.0, StatusCode::FORBIDDEN);

    let old = seal_area_key(&key, area, 0, &b.devices[0].cert).unwrap();
    let stranger = seal_area_key(&key, area, 1, &c.devices[0].cert).unwrap();
    for bad in [old, stranger] {
        let (status, body) = env.authed(&ta, Method::PUT, &put, Some(json!({ "envelopes": [bad] }))).await;
        assert_eq!(code(status, &body), (StatusCode::BAD_REQUEST, Some("invalid")));
    }
}

#[tokio::test]
async fn member_cannot_upload_envelopes_or_delete() {
    let env = TestEnv::new().await;
    let (a, b, _, ta, tb, _) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![member(&b, Role::Write)])).await;
    let for_b = seal_area_key(&key, area, 1, &b.devices[0].cert).unwrap();
    let (status, _) = env.authed(&tb, Method::PUT, &format!("/v1/areas/{}/envelopes", area.0), Some(json!({ "envelopes": [for_b] }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = env.authed(&tb, Method::DELETE, &format!("/v1/areas/{}", area.0), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn owner_deletes_the_area() {
    let env = TestEnv::new().await;
    let (a, b, _, ta, tb, _) = three(&env).await;
    let (area, key) = (AreaId(Uuid::new_v4()), AreaKey::generate());
    put_manifest(&env, &ta, &manifest(&a, 0, area, 1, &key, vec![member(&b, Role::Read)])).await;
    let path = format!("/v1/areas/{}", area.0);
    assert_eq!(env.authed(&ta, Method::DELETE, &path, None).await.0, StatusCode::OK);
    let (_, list) = env.authed(&tb, Method::GET, "/v1/areas", None).await;
    assert_eq!(list["areas"], json!([]));
    let envelope = format!("/v1/areas/{}/envelope", area.0);
    assert_eq!(env.authed(&tb, Method::GET, &envelope, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(env.authed(&ta, Method::DELETE, &path, None).await.0, StatusCode::NOT_FOUND);
}
