use sshcraft_server_lib::config::ServeConfig;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn serve_starts_answers_and_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        listen: addr,
        database_url: format!("sqlite://{}", dir.path().join("serve.db").display()),
        public_origin: "https://sync.example.org".into(),
        ..ServeConfig::default()
    };
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(sshcraft_server_lib::serve_on(listener, config, async {
        let _ = stopped.await;
    }));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /v1/server HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"api\":1"), "{response}");

    stop.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("serve returns after shutdown")
        .unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn serve_refuses_a_missing_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = ServeConfig::default();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        sshcraft_server_lib::serve_on(listener, config, std::future::pending()),
    )
    .await
    .expect("returns at once");
    assert!(result.is_err());
}

#[tokio::test]
async fn health_check_reports_a_running_server() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        listen: addr,
        database_url: format!("sqlite://{}", dir.path().join("health.db").display()),
        public_origin: "https://sync.example.org".into(),
        ..ServeConfig::default()
    };
    tokio::spawn(sshcraft_server_lib::serve_on(
        listener,
        config,
        std::future::pending(),
    ));
    assert!(sshcraft_server_lib::health(addr).await.is_ok());
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    assert!(sshcraft_server_lib::health(closed).await.is_err());
}
