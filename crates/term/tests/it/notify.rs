//! Messages a server sends for the desktop (OSC 9, OSC 777) and program status (OSC 7501).

use sshcraft_term::{Notice, ProgramState};

use crate::term;

fn notices(bytes: &[u8]) -> Vec<Notice> {
    let mut t = term(80, 24);
    t.feed(bytes);
    t.take_notices()
}

fn notifications(bytes: &[u8]) -> Vec<(Option<String>, String)> {
    notices(bytes)
        .into_iter()
        .filter_map(|n| match n {
            Notice::Notification { title, body } => Some((title, body)),
            _ => None,
        })
        .collect()
}

#[test]
fn osc_9_and_777_are_notifications() {
    assert_eq!(notifications(b"\x1b]9;build done\x07"), vec![(None, "build done".to_string())]);
    assert_eq!(
        notifications(b"\x1b]777;notify;Deploy;finished; all good\x1b\\"),
        vec![(Some("Deploy".to_string()), "finished; all good".to_string())]
    );
}

#[test]
fn conemu_progress_is_no_notification() {
    assert!(notifications(b"\x1b]9;4;1;50\x07").is_empty());
}

#[test]
fn notification_text_is_bounded_and_plain() {
    let long = "x".repeat(5000);
    let got = notifications(format!("\x1b]777;notify;{long};a\u{202e}b\rc\x07").as_bytes());
    let (title, body) = got.first().cloned().expect("a notification");
    assert_eq!(title.map(|t| t.chars().count()), Some(128));
    assert_eq!(body, "abc");
}

fn status(t: &mut sshcraft_term::Terminal, body: &str) {
    t.feed(format!("\x1b]7501;{body}\x07").as_bytes());
}

#[test]
fn a_status_report_creates_a_record() {
    let mut t = term(80, 24);
    // "Reviewing" and "Allow edit?" in base64.
    status(&mut t, "state=blocked:id=agent/1:app=claude:kind=permission:title=UmV2aWV3aW5n:msg=QWxsb3cgZWRpdD8=");
    let records = t.program_status();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.state, ProgramState::Blocked);
    assert_eq!(r.id, "agent/1");
    assert_eq!(r.app.as_deref(), Some("claude"));
    assert_eq!(r.title.as_deref(), Some("Reviewing"));
    assert_eq!(r.msg.as_deref(), Some("Allow edit?"));
    assert!(t.take_notices().contains(&Notice::ProgramStatusChanged));
}

#[test]
fn a_broken_report_is_discarded_whole() {
    let mut t = term(80, 24);
    status(&mut t, "state=working:title=not*base64");
    status(&mut t, "state=working:averyveryverylongkey=1");
    status(&mut t, "state=working:id=bad id");
    status(&mut t, "state=done:progress=50");
    status(&mut t, "state=sleeping");
    assert!(t.program_status().is_empty(), "{:?}", t.program_status());
}

#[test]
fn clear_removes_an_id_and_everything_below_it() {
    let mut t = term(80, 24);
    for id in ["a", "a/b", "a/b/c", "z"] {
        status(&mut t, &format!("state=working:id={id}"));
    }
    status(&mut t, "state=clear:id=a/b");
    let ids: Vec<String> = t.program_status().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["a", "z"]);
    status(&mut t, "state=clear");
    assert!(t.program_status().is_empty());
}

#[test]
fn records_are_bounded() {
    let mut t = term(80, 24);
    for i in 0..300 {
        status(&mut t, &format!("state=working:id=p{i}"));
    }
    assert_eq!(t.program_status().len(), 256);
}

#[test]
fn a_prompt_drops_running_and_waiting_programs() {
    let mut t = term(80, 24);
    status(&mut t, "state=working:id=w");
    status(&mut t, "state=done:id=d");
    status(&mut t, "state=blocked:id=b");
    t.feed(b"\x1b]133;A\x07");
    let ids: Vec<String> = t.program_status().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["d"]);
    t.drop_finished();
    assert!(t.program_status().is_empty());
}

#[test]
fn a_full_reset_drops_everything() {
    let mut t = term(80, 24);
    status(&mut t, "state=done:id=d");
    t.feed(b"\x1bc");
    assert!(t.program_status().is_empty());
}

#[test]
fn the_summary_names_the_most_urgent_state() {
    let mut t = term(80, 24);
    assert_eq!(t.status_summary(), None);
    status(&mut t, "state=working:id=a");
    status(&mut t, "state=done:id=b");
    assert_eq!(t.status_summary(), Some(ProgramState::Done));
    status(&mut t, "state=error:id=c");
    assert_eq!(t.status_summary(), Some(ProgramState::Error));
    status(&mut t, "state=blocked:id=d");
    assert_eq!(t.status_summary(), Some(ProgramState::Blocked));
}

#[test]
fn the_capability_query_is_answered_with_its_own_terminator() {
    assert!(notices(b"\x1b]7501;?\x07").contains(&Notice::Reply(b"\x1b]7501;?\x07".to_vec())));
    assert!(notices(b"\x1b]7501;?\x1b\\").contains(&Notice::Reply(b"\x1b]7501;?\x1b\\".to_vec())));
}
