//! A test helper for proxy commands: connects to `<address> <port>` and joins the connection
//! with standard input and output, like `nc`. With `fail` as the only argument it writes to
//! standard error and exits. Built only with the `testing` feature.
#![allow(clippy::unwrap_used, clippy::expect_used)] // A test helper: a failure should be loud.

use tokio::io::{AsyncWriteExt, copy, stdin, stdout};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("fail") {
        eprintln!("pipe-test: no route to the bastion");
        std::process::exit(3);
    }
    let port: u16 = args[1].parse().expect("port");
    let tcp = tokio::net::TcpStream::connect((args[0].as_str(), port)).await.expect("connect");
    let (mut from, mut to) = tcp.into_split();
    let up = async {
        copy(&mut stdin(), &mut to).await.ok();
        to.shutdown().await.ok();
    };
    let down = async {
        let mut out = stdout();
        copy(&mut from, &mut out).await.ok();
        out.flush().await.ok();
    };
    tokio::join!(up, down);
}
