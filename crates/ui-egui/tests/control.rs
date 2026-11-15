//! The control channel in the window: requests answered in the frame, input that reaches the
//! widgets as a person's would, and parameters checked before anything happens.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use sshcraft_engine::SessionStatus;
use sshcraft_engine::testing::test_host;
use sshcraft_ssh::testing::TestAuth;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::control::{ControlQueue, ControlRequest, png_of};
use sshcraft_ui_egui::state::ConnectRequest;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

/// One frame, with the control channel's input in front of it (as eframe's raw-input hook does).
fn step(h: &mut H) {
    let events = h.state_mut().take_control_input();
    h.input_mut().events.extend(events);
    h.step();
}

fn request(h: &mut H, queue: &ControlQueue, method: &str, params: Value) -> Result<Value, String> {
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    queue.push(ControlRequest { method: method.into(), params, reply });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        step(h);
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        assert!(Instant::now() < deadline, "no answer to {method}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn with_control(h: &mut H) -> ControlQueue {
    let ctx = h.ctx.clone();
    let queue = h.state_mut().enable_control(&ctx);
    h.run_steps(2);
    queue
}

#[test]
fn ui_inspect_shows_the_page_and_its_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let state = request(&mut h, &queue, "ui.inspect", json!({})).unwrap();
    assert_eq!(state["page"], "hosts");
    assert_eq!(state["tabs"], json!([]));
    assert!(state["window"]["width"].as_f64().unwrap() > 100.0);
    request(&mut h, &queue, "ui.set", json!({ "page": "keys" })).unwrap();
    assert_eq!(request(&mut h, &queue, "ui.inspect", json!({})).unwrap()["page"], "keys");
}

#[test]
fn ui_click_presses_a_button() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let at = h.get_by_label("New host").rect().center();
    request(&mut h, &queue, "ui.click", json!({ "x": at.x, "y": at.y })).unwrap();
    h.run_steps(2);
    assert!(h.state().ui.host_form.is_some(), "the click went where a person's would");
}

#[test]
fn ui_text_and_keys_reach_the_terminal_and_terminal_read_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host) = session.runtime().block_on(test_host(&session, "web", TestAuth::default(), true));
    let queue = with_control(&mut h);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !h.state().terminals.values().any(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))) {
        assert!(Instant::now() < deadline, "no connection");
        std::thread::sleep(Duration::from_millis(20));
        step(&mut h);
    }
    request(&mut h, &queue, "ui.text", json!({ "text": "echo typed-by-control" })).unwrap();
    request(&mut h, &queue, "ui.key", json!({ "key": "Enter" })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let text = request(&mut h, &queue, "terminal.read", json!({})).unwrap();
        if text["text"].as_str().unwrap().contains("typed-by-control") {
            break;
        }
        assert!(Instant::now() < deadline, "{text}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn bad_ui_params_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let at = h.get_by_label("New host").rect().center();
    for (method, params) in [
        ("ui.click", json!({ "x": "a", "y": 1 })),
        ("ui.click", json!({ "x": at.x, "y": at.y, "z": 1 })),
        ("ui.click", json!({ "x": at.x, "y": at.y, "count": 100 })),
        ("ui.click", json!({ "x": 1e9, "y": 1 })),
        ("ui.click", json!({ "x": at.x, "y": at.y, "button": "fourth" })),
        ("ui.key", json!({ "key": "NoSuchKey" })),
        ("ui.key", json!({ "key": "A", "ctrl": "yes" })),
        ("ui.text", json!({ "text": "x".repeat(5000) })),
        ("ui.set", json!({ "page": "tab:9" })),
        ("ui.resize", json!({ "width": -1, "height": 10 })),
        ("ui.inspect", json!([1])),
        ("ui.unknown", json!({})),
    ] {
        assert!(request(&mut h, &queue, method, params.clone()).is_err(), "{method} {params}");
    }
    h.run_steps(3);
    assert!(h.state().ui.host_form.is_none(), "no bad click got through");
}

#[test]
fn a_screenshot_is_a_png() {
    let png = png_of(2, 2, vec![255; 16]).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let back = image::load_from_memory(&png).unwrap();
    assert_eq!((back.width(), back.height()), (2, 2));
    assert!(png_of(3, 3, vec![0; 4]).is_err(), "a size that does not fit its pixels");
}
