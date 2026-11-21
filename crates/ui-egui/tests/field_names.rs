//! Every text field on the pages and in the dialogs has a name a screen reader can say: a label
//! tied to it (`labelled_by`), its own label, or its hint (crate::field_names).

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::field_names::is_text_field;
use sshcraft_ui_egui::pages::{hosts, settings::Section};
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

/// The text fields on the screen without a name, as "where: role at place (hint)".
fn unnamed(h: &H, place: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![h.root().accesskit_node()];
    while let Some(node) = stack.pop() {
        if is_text_field(node.role()) && !node.has_label() {
            let at = node.bounding_box().map(|r| format!("{:.0},{:.0}", r.x0, r.y0)).unwrap_or_default();
            out.push(format!("{place}: {:?} at {at} ({:?})", node.role(), node.data().placeholder()));
        }
        stack.extend(node.children());
    }
    out
}

/// The names of the text fields on the screen, as a screen reader says them.
fn names(h: &H) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![h.root().accesskit_node()];
    while let Some(node) = stack.pop() {
        if is_text_field(node.role()) {
            out.push(node.label().unwrap_or_default());
        }
        stack.extend(node.children());
    }
    out
}

fn show(h: &mut H, page: Page) {
    h.state_mut().ui.page = page;
    h.run_steps(3);
}

fn open(h: &mut H, button: &str) {
    h.get_by_label(button).click();
    h.run_steps(3);
}

#[test]
fn every_text_field_has_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    let mut missing = Vec::new();

    show(&mut h, Page::Hosts);
    missing.extend(unnamed(&h, "hosts"));
    assert!(names(&h).contains(&"Search hosts".to_string()), "a hint names the search: {:?}", names(&h));
    hosts::new_host(h.state_mut());
    h.run_steps(3);
    missing.extend(unnamed(&h, "host form"));
    // A label beside a field names it, before the field's hint ("comma separated").
    for name in ["Name", "Tags", "Address", "Port", "User"] {
        assert!(names(&h).contains(&name.to_string()), "{name} in {:?}", names(&h));
    }
    h.state_mut().ui.host_form = None;
    h.run_steps(2);
    open(&mut h, "New group");
    missing.extend(unnamed(&h, "group form"));
    h.state_mut().ui.group_form = None;

    show(&mut h, Page::Keys);
    missing.extend(unnamed(&h, "keys"));
    for (button, what) in [("Generate key", "generate key"), ("Import key", "import key")] {
        open(&mut h, button);
        missing.extend(unnamed(&h, what));
        assert!(names(&h).contains(&"Passphrase".to_string()), "{what}: {:?}", names(&h));
        h.state_mut().ui.key_dialog = None;
        h.run_steps(2);
    }
    open(&mut h, "New identity");
    missing.extend(unnamed(&h, "identity form"));
    h.state_mut().ui.identity_form = None;

    show(&mut h, Page::Snippets);
    missing.extend(unnamed(&h, "snippets"));
    open(&mut h, "New snippet");
    missing.extend(unnamed(&h, "snippet form"));
    h.state_mut().ui.snippet_form = None;

    show(&mut h, Page::Forwarding);
    missing.extend(unnamed(&h, "forwarding"));
    open(&mut h, "New rule");
    missing.extend(unnamed(&h, "forwarding form"));
    h.state_mut().ui.forward_form = None;

    show(&mut h, Page::Logs);
    missing.extend(unnamed(&h, "logs"));

    show(&mut h, Page::Settings);
    for section in
        [Section::General, Section::Appearance, Section::Terminal, Section::Shortcuts, Section::Sync, Section::Agents, Section::Connections]
    {
        h.state_mut().ui.settings.section = section;
        h.run_steps(3);
        missing.extend(unnamed(&h, &format!("settings {section:?}")));
    }

    assert!(missing.is_empty(), "text fields without a name:\n{}", missing.join("\n"));
}
