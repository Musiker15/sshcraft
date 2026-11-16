//! Macros: named sequences of steps (wait for a text, then send a line) from the settings,
//! run on the focused terminal from the command palette. A step without a text to wait for sends
//! at once. Like a login script, a step that waits longer than 15 seconds stops the macro; the
//! terminal says so. What a macro sends goes like typing, so broadcast applies.

use std::time::Instant;

use sshcraft_engine::SessionId;
use sshcraft_engine::login_script::{Feed, LoginScript};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};

/// A macro running on one terminal.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_text_reads_back_as_the_same_steps() {
        let steps = parse_steps("cd /srv\nready => make deploy\n\n$ =>  exit ");
        assert_eq!(steps.len(), 3);
        assert_eq!((steps[0].expect.as_str(), steps[0].send.as_str()), ("", "cd /srv"));
        assert_eq!((steps[1].expect.as_str(), steps[1].send.as_str()), ("ready", "make deploy"));
        assert_eq!((steps[2].expect.as_str(), steps[2].send.as_str()), ("$", "exit"));
        assert_eq!(parse_steps(&steps_text(&steps)), steps);
    }
}

#[derive(Debug)]
pub struct MacroRun {
    pub name: String,
    script: LoginScript,
}

/// Starts the macro `name` on terminal `id`; one running there stops.
pub fn start(app: &mut SshcraftApp, id: SessionId, name: &str) {
    let Some(m) = app.data.settings.terminal.macros.iter().find(|m| m.name == name).cloned() else { return };
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    tab.macro_run = Some(MacroRun { name: m.name, script: LoginScript::new(m.steps, Instant::now()) });
    // Steps that wait for nothing go at once.
    tick(app, id, &[]);
}

/// Stops the macro on terminal `id`.
pub fn stop(app: &mut SshcraftApp, id: SessionId) {
    if let Some(tab) = app.terminals.get_mut(&id)
        && let Some(run) = tab.macro_run.take()
    {
        app.toasts.info(&trf("Macro {name} stopped.", &[("name", &crate::widgets::visible(&run.name))]));
    }
}

/// Output of terminal `id` (or nothing, once a frame): the macro takes its next step when its
/// text came, and stops after the last one or when a step waited too long.
pub fn tick(app: &mut SshcraftApp, id: SessionId, bytes: &[u8]) {
    let mut sends = Vec::new();
    let mut timed_out = None;
    if let Some(tab) = app.terminals.get_mut(&id)
        && let Some(run) = tab.macro_run.as_mut()
    {
        // Several steps may be ready at once (each that waits for nothing).
        let mut feed = bytes;
        loop {
            match run.script.feed(feed, Instant::now()) {
                Feed::Send(line) => sends.push(line),
                Feed::TimedOut => {
                    timed_out = Some(run.name.clone());
                    break;
                }
                Feed::Nothing => break,
            }
            feed = &[];
        }
        if timed_out.is_some() || run.script.finished() {
            tab.macro_run = None;
        }
    }
    for line in sends {
        for target in crate::session_tab::broadcast_targets(app, id) {
            crate::session_tab::send_bytes(app, target, &line);
        }
    }
    if let Some(name) = timed_out {
        app.toasts.error(&trf("Macro {name} stopped: a step did not see its text within 15 seconds.", &[("name", &crate::widgets::visible(&name))]));
    }
}

/// The session of the focused pane in the shown tab.
pub fn focused_session(app: &SshcraftApp) -> Option<SessionId> {
    let crate::state::Page::Tab(i) = app.ui.page else { return None };
    match app.ui.tabs.get(i)?.panes.focused_slot()? {
        crate::state::PaneSlot::Session { session } => Some(session),
        _ => None,
    }
}

/// Steps from the editor's text: one per line, `text to wait for => line to send`, or just the
/// line to send at once. Blank lines are skipped.
pub fn parse_steps(text: &str) -> Vec<sshcraft_model::login_script::LoginStep> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| match line.split_once("=>") {
            Some((expect, send)) => sshcraft_model::login_script::LoginStep { expect: expect.trim().to_string(), send: send.trim().to_string() },
            None => sshcraft_model::login_script::LoginStep { expect: String::new(), send: line.trim().to_string() },
        })
        .collect()
}

/// The editor's text for `steps`.
pub fn steps_text(steps: &[sshcraft_model::login_script::LoginStep]) -> String {
    steps.iter().map(|s| if s.expect.is_empty() { s.send.clone() } else { format!("{} => {}", s.expect, s.send) }).collect::<Vec<_>>().join("\n")
}

/// The palette label of macro `name`.
pub fn label(name: &str) -> String {
    trf("Macro: {name}", &[("name", &crate::widgets::visible(name))])
}

/// What the stop command is called.
pub fn stop_label() -> &'static str {
    tr("Stop the macro")
}
