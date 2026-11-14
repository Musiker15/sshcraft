//! Questions to the user: whether to trust a host key, and sign-in questions (a password, codes
//! from keyboard-interactive). A question whose session goes away is withdrawn, so no dialog is
//! left behind for a closed tab. Open questions can always be listed again (`prompt.list`), so a
//! subscriber that fell behind on the event bus still finds them.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use sshcraft_ssh::{AuthPromptLine, AuthPrompter, AuthRequest};
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::events::{EngineEvent, EventBus};
use crate::sessions::HostKeyPrompter;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostKeyPrompt {
    pub prompt_id: u64,
    pub host_name: String,
    pub address: String,
    pub port: u16,
    pub fingerprint: String,
}

/// A sign-in question. Carries the questions only, never an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthPrompt {
    pub prompt_id: u64,
    pub host_name: String,
    pub username: String,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<AuthPromptLine>,
}

/// A question that waits for an answer, as `prompt.list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenPrompt {
    HostKey(HostKeyPrompt),
    Auth(AuthPrompt),
}

impl OpenPrompt {
    pub fn prompt_id(&self) -> u64 {
        match self {
            Self::HostKey(p) => p.prompt_id,
            Self::Auth(p) => p.prompt_id,
        }
    }
}

/// Delivers questions to the user (the event bus in the engine, a recorder in tests).
pub trait PromptEmitter: Send + Sync + 'static {
    /// Shows the question; `false` if it could not be delivered.
    fn asked(&self, prompt: &HostKeyPrompt) -> bool;
    /// Shows a sign-in question; `false` if it could not be delivered.
    fn asked_auth(&self, prompt: &AuthPrompt) -> bool;
    /// Takes back a question nobody waits for anymore (ids are unique across both kinds).
    fn withdrawn(&self, prompt_id: u64);
}

/// Sends questions on the engine's event bus. Without a subscriber nobody can answer, so the
/// question is not asked at all (an unknown host key is then not trusted).
pub struct BusEmitter(pub EventBus);

impl PromptEmitter for BusEmitter {
    fn asked(&self, prompt: &HostKeyPrompt) -> bool {
        if !self.0.has_subscribers() {
            return false;
        }
        self.0.send(EngineEvent::HostKeyPrompt(prompt.clone()));
        true
    }

    fn asked_auth(&self, prompt: &AuthPrompt) -> bool {
        if !self.0.has_subscribers() {
            return false;
        }
        self.0.send(EngineEvent::AuthPrompt(prompt.clone()));
        true
    }

    fn withdrawn(&self, prompt_id: u64) {
        self.0.send(EngineEvent::PromptWithdrawn { prompt_id });
    }
}

type Answers = Option<Vec<Zeroizing<String>>>;

enum Answer {
    HostKey(oneshot::Sender<bool>),
    Auth(oneshot::Sender<Answers>),
}

struct Pending {
    prompt: OpenPrompt,
    answer: Answer,
}

/// Open questions and their answer channels. No answer counts as "no" (or "cancel").
pub struct PromptRegistry<E: PromptEmitter> {
    emitter: E,
    next_id: AtomicU64,
    pending: Mutex<BTreeMap<u64, Pending>>,
}

/// Removes and withdraws a question when `ask` is dropped before it was answered.
struct Withdraw<'a, E: PromptEmitter> {
    registry: &'a PromptRegistry<E>,
    prompt_id: u64,
}

impl<E: PromptEmitter> Drop for Withdraw<'_, E> {
    fn drop(&mut self) {
        if self.registry.take(self.prompt_id).is_some() {
            self.registry.emitter.withdrawn(self.prompt_id);
        }
    }
}

impl<E: PromptEmitter> PromptRegistry<E> {
    pub fn new(emitter: E) -> Self {
        Self { emitter, next_id: AtomicU64::new(1), pending: Mutex::new(BTreeMap::new()) }
    }

    pub fn emitter(&self) -> &E {
        &self.emitter
    }

    fn pending(&self) -> MutexGuard<'_, BTreeMap<u64, Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn pending_count(&self) -> usize {
        self.pending().len()
    }

    /// A new question id. Every kind of question (also the agents' confirmations) draws from
    /// this one count, so an id never answers a question of another kind.
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Every question that waits for an answer, oldest first.
    pub fn open_prompts(&self) -> Vec<OpenPrompt> {
        self.pending().values().map(|p| p.prompt.clone()).collect()
    }

    fn take(&self, prompt_id: u64) -> Option<Pending> {
        self.pending().remove(&prompt_id)
    }

    fn put_back(&self, prompt_id: u64, pending: Pending) {
        self.pending().insert(prompt_id, pending);
    }

    /// Answers a host key question; returns whether one with this id was open.
    pub fn answer(&self, prompt_id: u64, accept: bool) -> bool {
        match self.take(prompt_id) {
            Some(Pending { answer: Answer::HostKey(tx), .. }) => {
                let _ = tx.send(accept);
                true
            }
            // Not a host key question: it stays open.
            Some(other) => {
                self.put_back(prompt_id, other);
                false
            }
            None => false,
        }
    }

    /// The user's answers to a sign-in question; `None` cancels. Returns whether one with this id
    /// was open.
    pub fn answer_auth(&self, prompt_id: u64, answers: Option<Vec<String>>) -> bool {
        let answers = answers.map(|a| a.into_iter().map(Zeroizing::new).collect());
        match self.take(prompt_id) {
            Some(Pending { answer: Answer::Auth(tx), .. }) => {
                let _ = tx.send(answers);
                true
            }
            Some(other) => {
                self.put_back(prompt_id, other);
                false
            }
            None => false,
        }
    }

    pub async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        let prompt_id = self.next_id();
        let (tx, rx) = oneshot::channel();
        let prompt =
            HostKeyPrompt { prompt_id, host_name: host_name.to_owned(), address: address.to_owned(), port, fingerprint: fingerprint.to_owned() };
        self.put_back(prompt_id, Pending { prompt: OpenPrompt::HostKey(prompt.clone()), answer: Answer::HostKey(tx) });
        if !self.emitter.asked(&prompt) {
            self.take(prompt_id);
            return false;
        }
        let _withdraw = Withdraw { registry: self, prompt_id };
        rx.await.unwrap_or(false)
    }

    pub async fn ask_auth(&self, request: AuthRequest) -> Answers {
        let prompt_id = self.next_id();
        let (tx, rx) = oneshot::channel();
        let prompt = AuthPrompt {
            prompt_id,
            host_name: request.host_name,
            username: request.username,
            name: request.name,
            instructions: request.instructions,
            prompts: request.prompts,
        };
        self.put_back(prompt_id, Pending { prompt: OpenPrompt::Auth(prompt.clone()), answer: Answer::Auth(tx) });
        if !self.emitter.asked_auth(&prompt) {
            self.take(prompt_id);
            return None;
        }
        let _withdraw = Withdraw { registry: self, prompt_id };
        rx.await.unwrap_or(None)
    }
}

#[async_trait::async_trait]
impl<E: PromptEmitter> HostKeyPrompter for PromptRegistry<E> {
    async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        PromptRegistry::ask(self, host_name, address, port, fingerprint).await
    }
}

#[async_trait::async_trait]
impl<E: PromptEmitter> AuthPrompter for PromptRegistry<E> {
    async fn answer(&self, request: AuthRequest) -> Answers {
        self.ask_auth(request).await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Default)]
    pub(crate) struct FakeEmitter {
        pub asked: Mutex<Vec<u64>>,
        pub auth: Mutex<Vec<AuthPrompt>>,
        pub withdrawn: Mutex<Vec<u64>>,
    }

    impl PromptEmitter for FakeEmitter {
        fn asked(&self, prompt: &HostKeyPrompt) -> bool {
            self.asked.lock().unwrap().push(prompt.prompt_id);
            true
        }
        fn asked_auth(&self, prompt: &AuthPrompt) -> bool {
            self.auth.lock().unwrap().push(prompt.clone());
            true
        }
        fn withdrawn(&self, prompt_id: u64) {
            self.withdrawn.lock().unwrap().push(prompt_id);
        }
    }

    async fn first_question(registry: &PromptRegistry<FakeEmitter>) -> u64 {
        for _ in 0..200 {
            if let Some(id) = registry.emitter().asked.lock().unwrap().first() {
                return *id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no question was asked");
    }

    pub(crate) async fn first_auth_question(registry: &PromptRegistry<FakeEmitter>) -> AuthPrompt {
        for _ in 0..2000 {
            if let Some(p) = registry.emitter().auth.lock().unwrap().first() {
                return p.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no sign-in question was asked");
    }

    fn request() -> AuthRequest {
        AuthRequest {
            host_name: "web".into(),
            username: "root".into(),
            name: String::new(),
            instructions: String::new(),
            prompts: vec![AuthPromptLine { text: "Code: ".into(), echo: true }],
            passphrase_for: None,
        }
    }

    #[tokio::test]
    async fn dropped_question_is_withdrawn() {
        let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
        let asking = registry.clone();
        let task = tokio::spawn(async move { asking.ask("web", "10.0.0.1", 22, "SHA256:x").await });
        let id = first_question(&registry).await;
        task.abort();
        let _ = task.await;
        assert_eq!(*registry.emitter().withdrawn.lock().unwrap(), vec![id]);
        registry.answer(id, true);
        assert_eq!(registry.pending_count(), 0);
    }

    #[tokio::test]
    async fn answered_question_is_not_withdrawn() {
        let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
        let asking = registry.clone();
        let task = tokio::spawn(async move { asking.ask("web", "10.0.0.1", 22, "SHA256:x").await });
        let id = first_question(&registry).await;
        registry.answer(id, true);
        assert!(task.await.unwrap());
        assert!(registry.emitter().withdrawn.lock().unwrap().is_empty());
        assert_eq!(registry.pending_count(), 0);
    }

    #[tokio::test]
    async fn auth_answers_arrive() {
        let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
        let asking = registry.clone();
        let task = tokio::spawn(async move { asking.ask_auth(request()).await });
        let prompt = first_auth_question(&registry).await;
        assert_eq!(prompt.prompts[0].text, "Code: ");
        registry.answer_auth(prompt.prompt_id, Some(vec!["42".into()]));
        let answers = task.await.unwrap().unwrap();
        assert_eq!(answers[0].as_str(), "42");
    }

    #[tokio::test]
    async fn host_key_answer_does_not_answer_auth() {
        let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
        let asking = registry.clone();
        let task = tokio::spawn(async move { asking.ask_auth(request()).await });
        let prompt = first_auth_question(&registry).await;
        registry.answer(prompt.prompt_id, true);
        assert_eq!(registry.pending_count(), 1);
        registry.answer_auth(prompt.prompt_id, None);
        assert!(task.await.unwrap().is_none());
    }

    #[tokio::test]
    async fn dropped_auth_question_is_withdrawn() {
        let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
        let asking = registry.clone();
        let task = tokio::spawn(async move { asking.ask_auth(request()).await });
        let prompt = first_auth_question(&registry).await;
        task.abort();
        let _ = task.await;
        assert_eq!(*registry.emitter().withdrawn.lock().unwrap(), vec![prompt.prompt_id]);
    }
}
