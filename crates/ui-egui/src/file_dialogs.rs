//! File dialogs: craft-dialogs where the system has native ones, and a stand-in that answers every
//! request as unavailable where it has none (Android). The pages call this module only, so they
//! read the same on every platform.

#[cfg(not(target_os = "android"))]
pub use craft_dialogs::{Answer, Dialogs, FileDialog};
#[cfg(target_os = "android")]
pub use unavailable::{Answer, Dialogs, FileDialog};

/// The stand-in. Compiled everywhere so its tests run on the desktop too.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub mod unavailable {
    use std::path::PathBuf;
    use std::sync::Arc;

    /// What a dialog answered. Only `Unavailable` is ever produced here; the other variants keep
    /// the callers' matches the same as with craft-dialogs.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Answer {
        Cancelled,
        Paths(Vec<PathBuf>),
        Message(()),
        Unavailable(String),
    }

    /// A file dialog request. The builder methods only keep the call sites unchanged.
    #[derive(Debug, Clone, Default)]
    pub struct FileDialog;

    impl FileDialog {
        pub fn open() -> Self {
            Self
        }
        pub fn folder() -> Self {
            Self
        }
        pub fn save() -> Self {
            Self
        }
        #[must_use]
        pub fn title(self, _title: impl Into<String>) -> Self {
            self
        }
        #[must_use]
        pub fn filter(self, _name: impl Into<String>, _extensions: &[&str]) -> Self {
            self
        }
        #[must_use]
        pub fn file_name(self, _file_name: impl Into<String>) -> Self {
            self
        }
    }

    /// Queues the answers: every request is answered as unavailable on the next call to
    /// [`Dialogs::take_answers`], so the page that asked reports it like any failed dialog.
    pub struct Dialogs<T> {
        answers: Vec<(T, Answer)>,
        wake: Arc<dyn Fn() + Send + Sync>,
    }

    impl<T> Dialogs<T> {
        pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
            Self { answers: Vec::new(), wake }
        }

        pub fn ask(&mut self, _dialog: FileDialog, then: T) {
            self.answers.push((then, Answer::Unavailable(crate::i18n::tr("Files cannot be picked on this device.").to_string())));
            (self.wake)();
        }

        pub fn show_pending<W>(&mut self, _parent: Option<&W>) {}

        pub fn take_answers(&mut self) -> Vec<(T, Answer)> {
            std::mem::take(&mut self.answers)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[test]
        fn the_stand_in_answers_every_request_as_unavailable() {
            let woken = Arc::new(AtomicUsize::new(0));
            let count = woken.clone();
            let mut dialogs = Dialogs::new(Arc::new(move || {
                count.fetch_add(1, Ordering::Relaxed);
            }));
            dialogs.ask(FileDialog::open().title("Import"), 1);
            dialogs.ask(FileDialog::save().file_name("x").filter("JSON", &["json"]), 2);
            let answers = dialogs.take_answers();
            assert_eq!(answers.len(), 2);
            assert!(answers.iter().all(|(_, a)| matches!(a, Answer::Unavailable(why) if !why.is_empty())));
            assert_eq!(answers.iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![1, 2]);
            assert_eq!(woken.load(Ordering::Relaxed), 2, "the window wakes to report the answer");
            assert!(dialogs.take_answers().is_empty(), "an answer is taken once");
        }
    }
}
