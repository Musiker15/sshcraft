//! Dialogs and the commands they send. A dialog sends one command at a time (its buttons wait
//! until the answer is back), and an answer that comes after its dialog was closed, or replaced
//! by another, never touches the dialog open now.

use std::hash::{Hash, Hasher};

use crate::state::UiState;

/// Which dialogs are open, and the command a dialog is waiting for.
#[derive(Debug, Default)]
pub struct Forms {
    /// Counts the changes of which dialogs are open.
    generation: u64,
    shape: u64,
    /// The generation whose dialog sent a command that has not answered yet.
    pending: Option<u64>,
}

impl Forms {
    /// Called after every frame with [`shape`]: a dialog opened, closed or replaced starts a new
    /// generation.
    pub fn observe(&mut self, shape: u64) {
        if shape != self.shape {
            self.shape = shape;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// The open dialog waits for its command.
    pub fn busy(&self) -> bool {
        self.pending == Some(self.generation)
    }

    /// Marks the open dialog as waiting and returns its generation; `None` while it already waits.
    pub fn start(&mut self) -> Option<u64> {
        if self.busy() {
            return None;
        }
        self.pending = Some(self.generation);
        Some(self.generation)
    }

    /// The command of `generation` answered; whether its dialog is still the one open.
    pub fn done(&mut self, generation: u64) -> bool {
        if self.pending == Some(generation) {
            self.pending = None;
        }
        generation == self.generation
    }
}

/// Which dialogs are open (by kind, not by content).
pub fn shape(ui: &UiState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        ui.host_form.is_some(),
        ui.group_form.is_some(),
        ui.key_dialog.as_ref().map(std::mem::discriminant),
        ui.identity_form.is_some(),
        ui.passcli.is_some(),
        ui.import.is_some(),
        ui.export.is_some(),
        ui.confirm.is_some(),
        ui.key_confirm.is_some(),
    )
        .hash(&mut hasher);
    hasher.finish()
}
