#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use sshcraft_server::{AppState, app};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

pub const ORIGIN: &str = "https://sync.example.org";

pub struct TestEnv {
    pub state: AppState,
}

impl TestEnv {
    pub async fn new() -> Self {
        let store = Arc::new(sshcraft_server::store::NullStore);
        let state = AppState::new(store, ORIGIN.to_owned(), Duration::from_secs(30));
        Self { state }
    }

    /// Sends one request through the router and returns the status and the JSON body
    /// (`Value::Null` when the body is empty or not JSON).
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("request");
        let response = app(self.state.clone())
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }
}
