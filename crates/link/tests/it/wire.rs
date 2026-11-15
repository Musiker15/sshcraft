//! The messages on the wire and the addresses a client takes.

use serde_json::json;
use sshcraft_link::{Client, LinkError, Request, Response, Token};

#[test]
fn requests_and_responses_read_as_documented() {
    let request: Request = serde_json::from_str(r#"{"id": 7, "method": "ping"}"#).unwrap();
    assert_eq!(request.id, 7);
    assert_eq!(request.params, json!(null), "params may be left out");
    let ok = serde_json::to_value(Response::ok(7, json!(1))).unwrap();
    assert_eq!(ok, json!({"id": 7, "ok": true, "result": 1}));
    let error = serde_json::to_value(Response::error(7, "no")).unwrap();
    assert_eq!(error, json!({"id": 7, "ok": false, "error": "no"}));
}

#[tokio::test]
async fn only_loopback_addresses_are_taken() {
    let token = Token::generate();
    let start = std::time::Instant::now();
    let result = Client::connect("192.0.2.1:9".parse().unwrap(), &token).await;
    assert!(matches!(result, Err(LinkError::Refused(_))), "{result:?}");
    assert!(start.elapsed() < std::time::Duration::from_secs(1), "no attempt was made");
}
