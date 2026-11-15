//! Text from outside: what `visible` shows, placeholders filled in once, and open questions
//! merged after events were lost.

use serde_json::json;
use sshcraft_ui_egui::i18n::{Lang, set_language, trf};
use sshcraft_ui_egui::prompts::{PromptView, Prompts};
use sshcraft_ui_egui::widgets::{visible, visible_lines};

#[test]
fn every_bidi_control_is_made_visible() {
    assert_eq!(visible("a\u{61C}b"), "a⟨U+061C⟩b", "the Arabic letter mark");
    assert_eq!(visible("a\u{202E}b"), "a⟨U+202E⟩b");
    assert_eq!(visible("a\nb\tc"), "abc", "one line");
}

#[test]
fn instructions_keep_their_lines() {
    assert_eq!(visible_lines("first\nsecond\u{7}\r\n\u{202E}x"), "first\nsecond\n⟨U+202E⟩x");
}

#[test]
fn a_value_never_fills_a_later_placeholder() {
    set_language(Lang::En);
    let text = trf(
        "{host} ({address}) is not known yet. Compare the fingerprint with the one its administrator gave you before you accept it.",
        &[("host", "{address}"), ("address", "192.0.2.1:22")],
    );
    assert!(text.starts_with("{address} (192.0.2.1:22)"), "{text}");
}

fn prompt(id: u64) -> PromptView {
    PromptView::from_json(&json!({ "kind": "host_key", "prompt_id": id, "host_name": "h", "address": "a", "port": 22, "fingerprint": "f" })).unwrap()
}

#[test]
fn a_question_that_came_during_the_refresh_is_kept() {
    let mut prompts = Prompts::default();
    prompts.add(prompt(1));
    prompts.add(prompt(2));
    // The list was asked for when 2 was the newest; 3 came by event before the answer.
    let asked_after = prompts.newest();
    prompts.add(prompt(3));
    prompts.merge_listed(vec![prompt(2)], asked_after);
    let ids: Vec<u64> = prompts.open.iter().map(|p| p.id).collect();
    assert_eq!(ids, [2, 3], "1 was answered elsewhere, 3 is new");
}
