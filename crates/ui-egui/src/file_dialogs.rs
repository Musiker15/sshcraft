//! File dialogs: craft-dialogs where the system has native ones, and on Android the system's own
//! picker through the app (`picked`); a stand-in answers every request as unavailable where
//! there is neither. The pages call this module only, so they read the same on every platform.

#[cfg(not(target_os = "android"))]
pub use craft_dialogs::{Answer, Dialogs, FileDialog};
#[cfg(target_os = "android")]
pub use picked::{Answer, Dialogs, FileDialog};

/// Whether the pages offer what reads or writes a file (import, export, backups): on the desktop,
/// and on Android once the app has put the system's picker in place.
pub fn can_pick(app: &crate::app::SshcraftApp) -> bool {
    #[cfg(target_os = "android")]
    {
        app.dialogs.has_picker()
    }
    #[cfg(not(target_os = "android"))]
    {
        app.form == crate::app::Form::Desktop
    }
}

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

/// Android's file dialogs: the system's picker, which the app hands in ([`Dialogs::set_picker`]).
/// It answers later, with a path the app can read or write (a file descriptor the picker opened).
/// Folders cannot be picked as paths there. Compiled everywhere so its tests run on the desktop.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub mod picked {
    use std::path::PathBuf;
    use std::sync::Arc;

    pub use super::unavailable::Answer;

    /// What is asked for.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum Kind {
        #[default]
        Open,
        Save,
        Folder,
    }

    /// A file dialog request.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct FileDialog {
        pub kind: Kind,
        pub file_name: Option<String>,
        pub extensions: Vec<String>,
    }

    impl FileDialog {
        pub fn open() -> Self {
            Self { kind: Kind::Open, ..Self::default() }
        }
        pub fn folder() -> Self {
            Self { kind: Kind::Folder, ..Self::default() }
        }
        pub fn save() -> Self {
            Self { kind: Kind::Save, ..Self::default() }
        }
        #[must_use]
        pub fn title(self, _title: impl Into<String>) -> Self {
            self
        }
        #[must_use]
        pub fn filter(mut self, _name: impl Into<String>, extensions: &[&str]) -> Self {
            self.extensions.extend(extensions.iter().map(|e| (*e).to_string()));
            self
        }
        #[must_use]
        pub fn file_name(mut self, file_name: impl Into<String>) -> Self {
            self.file_name = Some(file_name.into());
            self
        }
    }

    /// The media type the picker shows for `extensions`: JSON and CSV by name, anything else for
    /// the rest (Android knows no type for `known_hosts`, keys or backups).
    pub fn mime_for(extensions: &[String]) -> &'static str {
        match extensions.first().map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("json") => "application/json",
            Some("csv") => "text/csv",
            _ => "*/*",
        }
    }

    /// One answer of the activity, `id<TAB>path`; an empty path means cancelled. None for a line
    /// that is not one.
    pub fn parse_answer(line: &str) -> Option<(u64, Option<PathBuf>)> {
        let (id, path) = line.split_once('\t')?;
        let id = id.trim().parse::<u64>().ok()?;
        Some((id, (!path.is_empty()).then(|| PathBuf::from(path))))
    }

    /// The system's picker. `ask` starts it for request `id`; `answers` hands back what was
    /// picked since (`None`: cancelled).
    pub trait Picker: Send + Sync {
        fn ask(&self, id: u64, dialog: &FileDialog) -> Result<(), String>;
        fn answers(&self) -> Vec<(u64, Option<PathBuf>)>;
    }

    pub struct Dialogs<T> {
        picker: Option<Arc<dyn Picker>>,
        waiting: Vec<(u64, T)>,
        answers: Vec<(T, Answer)>,
        next: u64,
        wake: Arc<dyn Fn() + Send + Sync>,
    }

    impl<T> Dialogs<T> {
        pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
            Self { picker: None, waiting: Vec::new(), answers: Vec::new(), next: 1, wake }
        }

        /// Puts the system's picker in place; without one every request is unavailable.
        pub fn set_picker(&mut self, picker: Arc<dyn Picker>) {
            self.picker = Some(picker);
        }

        pub fn has_picker(&self) -> bool {
            self.picker.is_some()
        }

        fn unavailable(&mut self, then: T, why: String) {
            self.answers.push((then, Answer::Unavailable(why)));
            (self.wake)();
        }

        pub fn ask(&mut self, dialog: FileDialog, then: T) {
            let Some(picker) = self.picker.clone() else {
                return self.unavailable(then, crate::i18n::tr("Files cannot be picked on this device.").to_string());
            };
            if dialog.kind == Kind::Folder {
                return self.unavailable(then, crate::i18n::tr("Folders cannot be picked on this device.").to_string());
            }
            let id = self.next;
            self.next = self.next.wrapping_add(1).max(1);
            match picker.ask(id, &dialog) {
                Ok(()) => self.waiting.push((id, then)),
                Err(e) => self.unavailable(then, e),
            }
        }

        pub fn show_pending<W>(&mut self, _parent: Option<&W>) {}

        /// What was answered: refusals, and what the picker handed back for the requests still
        /// waiting. An answer for no waiting request is dropped.
        pub fn take_answers(&mut self) -> Vec<(T, Answer)> {
            if let Some(picker) = &self.picker {
                for (id, path) in picker.answers() {
                    if let Some(at) = self.waiting.iter().position(|(w, _)| *w == id) {
                        let (_, then) = self.waiting.remove(at);
                        let answer = match path {
                            Some(path) => Answer::Paths(vec![path]),
                            None => Answer::Cancelled,
                        };
                        self.answers.push((then, answer));
                    }
                }
            }
            std::mem::take(&mut self.answers)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::Mutex;

        /// A picker that records what was asked and answers what a test gives it.
        #[derive(Default)]
        struct Fake {
            asked: Mutex<Vec<(u64, FileDialog)>>,
            answers: Mutex<Vec<(u64, Option<PathBuf>)>>,
        }

        impl Picker for Fake {
            fn ask(&self, id: u64, dialog: &FileDialog) -> Result<(), String> {
                self.asked.lock().unwrap().push((id, dialog.clone()));
                Ok(())
            }
            fn answers(&self) -> Vec<(u64, Option<PathBuf>)> {
                std::mem::take(&mut *self.answers.lock().unwrap())
            }
        }

        #[test]
        fn a_picked_file_reaches_the_request_that_asked() {
            let fake = Arc::new(Fake::default());
            let mut dialogs = Dialogs::new(Arc::new(|| {}));
            dialogs.set_picker(fake.clone());
            dialogs.ask(FileDialog::open().filter("JSON", &["json"]), "import");
            dialogs.ask(FileDialog::save().file_name("hosts.json"), "export");
            let asked = fake.asked.lock().unwrap().clone();
            assert_eq!(asked.len(), 2);
            assert_eq!(asked[1].1.file_name.as_deref(), Some("hosts.json"));
            assert!(dialogs.take_answers().is_empty(), "nothing is picked yet");
            fake.answers.lock().unwrap().extend([(asked[1].0, None), (asked[0].0, Some(PathBuf::from("/proc/self/fd/7"))), (99, None)]);
            let answers = dialogs.take_answers();
            assert_eq!(answers, vec![("export", Answer::Cancelled), ("import", Answer::Paths(vec![PathBuf::from("/proc/self/fd/7")]))]);
        }

        #[test]
        fn the_activitys_answers_and_media_types_are_read() {
            assert_eq!(parse_answer("7\t/proc/self/fd/42"), Some((7, Some(PathBuf::from("/proc/self/fd/42")))));
            assert_eq!(parse_answer("8\t"), Some((8, None)), "cancelled");
            assert_eq!(parse_answer("garbage"), None);
            assert_eq!(parse_answer("x\t/p"), None);
            assert_eq!(mime_for(&["JSON".into()]), "application/json");
            assert_eq!(mime_for(&["csv".into()]), "text/csv");
            assert_eq!(mime_for(&[]), "*/*");
        }

        #[test]
        fn folders_and_a_missing_picker_are_unavailable() {
            let mut dialogs = Dialogs::new(Arc::new(|| {}));
            dialogs.ask(FileDialog::open(), 1);
            assert!(matches!(dialogs.take_answers().as_slice(), [(1, Answer::Unavailable(_))]));
            dialogs.set_picker(Arc::new(Fake::default()));
            dialogs.ask(FileDialog::folder(), 2);
            assert!(matches!(dialogs.take_answers().as_slice(), [(2, Answer::Unavailable(_))]));
        }
    }
}
