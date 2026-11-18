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
fn ui_inspect_lists_the_widgets_a_script_can_click() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let state = request(&mut h, &queue, "ui.inspect", json!({})).unwrap();
    let widgets = state["widgets"].as_array().expect("a list of widgets");
    let button = widgets.iter().find(|w| w["label"] == "New host").expect("the New host button is listed");
    assert_eq!(button["role"], "Button");
    assert_eq!(button["enabled"], true);
    let (x, y) = (
        button["x"].as_f64().unwrap() + button["width"].as_f64().unwrap() / 2.0,
        button["y"].as_f64().unwrap() + button["height"].as_f64().unwrap() / 2.0,
    );
    request(&mut h, &queue, "ui.click", json!({ "x": x, "y": y })).unwrap();
    h.run_steps(2);
    assert!(h.state().ui.host_form.is_some(), "the listed rectangle is where the button is");
}

/// A pasted private key and a typed passphrase are on the screen, unmasked and masked; neither
/// reaches the channel, only the names of their fields do.
#[test]
fn ui_inspect_never_gives_a_pasted_key_or_a_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    request(&mut h, &queue, "ui.set", json!({ "page": "keys" })).unwrap();
    h.run_steps(3);
    h.get_by_label("Import key").click();
    h.run_steps(3);
    {
        let import = h.state_mut().ui.key_dialog.as_mut().and_then(sshcraft_ui_egui::pages::keys::KeyDialog::import_mut).expect("the dialog opened");
        import.text = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n".into();
        import.passphrase = "hunter2-correct-horse".into();
    }
    h.run_steps(3);
    let state = request(&mut h, &queue, "ui.inspect", json!({})).unwrap();
    let labels: Vec<&str> = state["widgets"].as_array().unwrap().iter().filter_map(|w| w["label"].as_str()).collect();
    assert!(labels.contains(&"Private key") && labels.contains(&"Passphrase"), "the fields are listed: {labels:?}");
    let answer = state.to_string();
    assert!(!answer.contains("PRIVATE KEY-----") && !answer.contains("b3BlbnNzaC1rZXkt"), "the key stays out of the answer");
    assert!(!answer.contains("hunter2"), "the passphrase stays out of the answer");
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

#[test]
fn a_click_whose_requester_went_away_never_lands() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let at = h.get_by_label("New host").rect().center();
    // Gone before the window took it (a script that timed out and was stopped).
    let (reply, answer) = tokio::sync::oneshot::channel();
    drop(answer);
    queue.push(ControlRequest { method: "ui.click".into(), params: json!({ "x": at.x, "y": at.y }), reply });
    for _ in 0..10 {
        step(&mut h);
    }
    assert!(h.state().ui.host_form.is_none(), "the stale click was dropped");
    // Gone while its input was going in: the rest of it is dropped.
    let (reply, answer) = tokio::sync::oneshot::channel();
    queue.push(ControlRequest { method: "ui.click".into(), params: json!({ "x": at.x, "y": at.y }), reply });
    step(&mut h);
    drop(answer);
    for _ in 0..10 {
        step(&mut h);
    }
    assert!(h.state().ui.host_form.is_none(), "the rest of the click was dropped");
}

#[test]
fn the_answer_comes_after_the_input_took_effect() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let at = h.get_by_label("New host").rect().center();
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    queue.push(ControlRequest { method: "ui.click".into(), params: json!({ "x": at.x, "y": at.y }), reply });
    for _ in 0..20 {
        let events = h.state_mut().take_control_input();
        if answer.try_recv().is_ok() {
            assert!(h.state().ui.host_form.is_some(), "answered before the click took effect");
            return;
        }
        h.input_mut().events.extend(events);
        h.step();
    }
    panic!("no answer");
}

#[test]
fn the_queue_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    let mut answers = Vec::new();
    for _ in 0..(sshcraft_ui_egui::control::MAX_QUEUED + 5) {
        let (reply, answer) = tokio::sync::oneshot::channel();
        queue.push(ControlRequest { method: "ui.inspect".into(), params: json!({}), reply });
        answers.push(answer);
    }
    let refused = answers.iter_mut().filter_map(|a| a.try_recv().ok()).filter(Result::is_err).count();
    assert_eq!(refused, 5, "what does not fit is refused at once");
}

/// While the window is locked the control channel tells nothing of the sessions or hosts, and
/// changes no page; the screenshot shows only the lock screen.
#[test]
fn the_locked_window_tells_the_channel_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let queue = with_control(&mut h);
    h.state_mut().lock.locked = true;
    step(&mut h);
    assert_eq!(request(&mut h, &queue, "ui.inspect", json!({})).unwrap(), json!({ "locked": true }));
    assert!(request(&mut h, &queue, "terminal.read", json!({})).is_err());
    assert!(request(&mut h, &queue, "ui.set", json!({ "page": "hosts" })).is_err());
    // Counter-check: unlocked, the page and tabs are there again.
    h.state_mut().lock.locked = false;
    step(&mut h);
    assert!(request(&mut h, &queue, "ui.inspect", json!({})).unwrap().get("page").is_some());
}
