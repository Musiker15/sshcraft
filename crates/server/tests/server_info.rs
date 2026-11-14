mod common;

use axum::http::{Method, StatusCode};
use common::TestEnv;
use sshcraft_server_lib::config::ServeConfig;

#[tokio::test]
async fn server_info_needs_no_login_and_names_the_origin() {
    let env = TestEnv::new().await;
    let (status, body) = env.request(Method::GET, "/v1/server", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["api"], 1);
    assert_eq!(body["origin"], "https://sync.example.org");
}

#[tokio::test]
async fn unknown_route_answers_in_the_error_format() {
    let env = TestEnv::new().await;
    let (status, body) = env.request(Method::GET, "/v1/nothing", &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

#[test]
fn config_accepts_only_http_origins() {
    let cfg = |o: &str| ServeConfig {
        public_origin: o.into(),
        ..ServeConfig::default()
    };
    assert_eq!(
        cfg("HTTPS://Sync.Example.org:443/x").validate().unwrap(),
        "https://sync.example.org"
    );
    assert!(cfg("sync.example.org").validate().is_err());
    assert!(cfg("ftp://sync.example.org").validate().is_err());
    assert!(cfg("https://").validate().is_err());
}

#[tokio::test]
async fn wrong_method_answers_in_the_error_format() {
    let env = TestEnv::new().await;
    let (status, body) = env.request(Method::DELETE, "/v1/server", &[], None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["code"], "method_not_allowed");
}

#[tokio::test]
async fn a_database_url_never_shows_up_in_errors() {
    for url in [
        "sshcraft:secretpw@localhost/db",
        "mysql://sshcraft:secretpw@localhost/db",
    ] {
        let Err(err) = sshcraft_server_lib::store::open(url).await else {
            panic!("{url} must not open");
        };
        assert!(!err.to_string().contains("secretpw"), "{err}");
    }
}
