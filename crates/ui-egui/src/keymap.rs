//! Keyboard shortcuts: the defaults of `docs/shortcuts.md`, own bindings from the settings
//! (`shortcuts.bindings`, by action name), key presses matched with craft-keymap's chords. Inside a terminal only
//! Ctrl+Shift (without Alt) and Ctrl+Tab are taken, so every other key reaches the server.

use std::collections::BTreeMap;

use craft_keymap::{Chord, Key, Mods};
use sshcraft_engine::settings::Shortcuts;

use crate::commands::{Scope, UiCommand, ui_commands};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// Not a key combination (the parser's reason).
    Invalid(String),
    /// A shortcut needs Ctrl, Alt or the Windows key, or is a function key.
    NeedsModifier,
    /// Another action has it; `owner` is that action's English label.
    Taken { owner: &'static str },
}

#[derive(Clone)]
pub struct Bindings {
    table: Vec<(&'static UiCommand, Vec<Chord>)>,
}

fn parse_all(texts: &[&str]) -> Vec<Chord> {
    texts.iter().filter_map(|t| Chord::parse(t).ok()).collect()
}

fn is_function_key(key: &Key) -> bool {
    let name = key.to_string();
    name.len() >= 2 && name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit())
}

/// egui's modifiers as the binding texts spell them (`Ctrl`, `Alt`, `Shift`, `Cmd` for ⌘).
/// craft-keymap's own reading folds Ctrl into its `cmd` off macOS, which a chord parsed from
/// `Ctrl+…` does not, so the two would never be equal.
fn chord_mods(m: &egui::Modifiers) -> Mods {
    Mods { cmd: m.mac_cmd, ctrl: m.ctrl, alt: m.alt, shift: m.shift }
}

/// Whether a terminal lets `chord` through to the interface (Ctrl+Shift without Alt, or Ctrl+Tab).
pub fn works_in_terminal(chord: &Chord) -> bool {
    let m = chord.mods;
    (m.ctrl && m.shift && !m.alt) || (m.ctrl && !m.alt && chord.key.to_string() == "Tab")
}

impl Default for Bindings {
    fn default() -> Self {
        Self { table: ui_commands().iter().map(|c| (c, parse_all(c.defaults))).collect() }
    }
}

impl Bindings {
    /// The defaults with the stored own bindings applied; unknown ids and unreadable texts are
    /// left out.
    pub fn from_settings(shortcuts: &Shortcuts) -> Self {
        let mut bindings = Self::default();
        for (command, chords) in &mut bindings.table {
            if let Some(stored) = shortcuts.bindings.get(command.action) {
                *chords = stored.iter().filter_map(|t| Chord::parse(t).ok()).collect();
            }
        }
        bindings
    }

    /// Own bindings only: what differs from the defaults.
    pub fn to_settings(&self) -> Shortcuts {
        let mut stored = BTreeMap::new();
        for (command, chords) in &self.table {
            if *chords != parse_all(command.defaults) {
                stored.insert(command.action.to_string(), chords.iter().map(ToString::to_string).collect());
            }
        }
        Shortcuts { bindings: stored }
    }

    pub fn chords(&self, id: &str) -> &[Chord] {
        self.table.iter().find(|(c, _)| c.id == id).map_or(&[], |(_, chords)| chords.as_slice())
    }

    /// The command `chord` runs, inside a terminal or not.
    pub fn command_for(&self, chord: &Chord, in_terminal: bool) -> Option<&'static str> {
        if in_terminal && !works_in_terminal(chord) {
            return None;
        }
        self.table.iter().filter(|(c, _)| c.scope == Scope::App || in_terminal).find(|(_, chords)| chords.contains(chord)).map(|(c, _)| c.id)
    }

    /// Makes `text` the one shortcut of `id`.
    pub fn assign(&mut self, id: &str, text: &str) -> Result<(), BindError> {
        let chord = Chord::parse(text).map_err(|e| BindError::Invalid(e.to_string()))?;
        let Mods { ctrl, alt, cmd, .. } = chord.mods;
        if !(ctrl || alt || cmd || is_function_key(&chord.key)) {
            return Err(BindError::NeedsModifier);
        }
        if let Some((owner, _)) = self.table.iter().find(|(c, chords)| c.id != id && chords.contains(&chord)) {
            return Err(BindError::Taken { owner: owner.label });
        }
        if let Some((_, chords)) = self.table.iter_mut().find(|(c, _)| c.id == id) {
            *chords = vec![chord];
        }
        Ok(())
    }

    /// Leaves `id` without a shortcut.
    pub fn remove(&mut self, id: &str) {
        if let Some((_, chords)) = self.table.iter_mut().find(|(c, _)| c.id == id) {
            chords.clear();
        }
    }

    /// Brings back the default shortcuts of `id`.
    pub fn reset(&mut self, id: &str) {
        if let Some((command, chords)) = self.table.iter_mut().find(|(c, _)| c.id == id) {
            *chords = parse_all(command.defaults);
        }
    }

    /// The commands this frame's key presses run, taking those presses out of the input so a
    /// widget (a terminal) does not get them as well.
    pub fn dispatch(&self, ctx: &egui::Context, in_terminal: bool) -> Vec<&'static str> {
        let mut hits = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
                let Some(key) = Key::from_egui(*key) else { return true };
                let chord = Chord { mods: chord_mods(modifiers), key };
                match self.command_for(&chord, in_terminal) {
                    Some(id) => {
                        hits.push(id);
                        false
                    }
                    None => true,
                }
            });
        });
        hits
    }
}
