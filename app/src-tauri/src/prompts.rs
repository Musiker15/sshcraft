//! Host key questions to the UI. A question whose session goes away is withdrawn, so no
//! dialog is left behind for a closed tab.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::oneshot;

use crate::sessions::HostKeyPrompter;

#[derive(Debug, Clone, Serialize)]
pub struct HostKeyPrompt {
    pub prompt_id: u64,
    pub host_name: String,
    pub address: String,
    pub port: u16,
    pub fingerprint: String,
}

/// Delivers questions to the UI (Tauri events in the app, a recorder in tests).
pub trait PromptEmitter: Send + Sync + 'static {
    /// Shows the question; `false` if it could not be delivered.
    fn asked(&self, prompt: &HostKeyPrompt) -> bool;
    /// Takes back a question nobody waits for anymore.
    fn withdrawn(&self, prompt_id: u64);
}

/// Open questions and their answer channels. No answer counts as "no".
pub struct PromptRegistry<E: PromptEmitter> {
    emitter: E,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
}

/// Removes and withdraws a question when `ask` is dropped before it was answered.
struct Withdraw<'a, E: PromptEmitter> {
    registry: &'a PromptRegistry<E>,
    prompt_id: u64,
}

impl<E: PromptEmitter> Drop for Withdraw<'_, E> {
    fn drop(&mut self) {
        let still_open = self
            .registry
            .pending
            .lock()
            .expect("pending lock")
            .remove(&self.prompt_id)
            .is_some();
        if still_open {
            self.registry.emitter.withdrawn(self.prompt_id);
        }
    }
}

impl<E: PromptEmitter> PromptRegistry<E> {
    pub fn new(emitter: E) -> Self {
        Self {
            emitter,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn emitter(&self) -> &E {
        &self.emitter
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().expect("pending lock").len()
    }

    pub fn answer(&self, prompt_id: u64, accept: bool) {
        let sender = self
            .pending
            .lock()
            .expect("pending lock")
            .remove(&prompt_id);
        if let Some(tx) = sender {
            let _ = tx.send(accept);
        }
    }

    pub async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        let prompt_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending lock")
            .insert(prompt_id, tx);
        let prompt = HostKeyPrompt {
            prompt_id,
            host_name: host_name.to_owned(),
            address: address.to_owned(),
            port,
            fingerprint: fingerprint.to_owned(),
        };
        if !self.emitter.asked(&prompt) {
            self.pending
                .lock()
                .expect("pending lock")
                .remove(&prompt_id);
            return false;
        }
        let _withdraw = Withdraw {
            registry: self,
            prompt_id,
        };
        rx.await.unwrap_or(false)
    }
}

#[async_trait::async_trait]
impl<E: PromptEmitter> HostKeyPrompter for PromptRegistry<E> {
    async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        PromptRegistry::ask(self, host_name, address, port, fingerprint).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Default)]
    struct FakeEmitter {
        asked: Mutex<Vec<u64>>,
        withdrawn: Mutex<Vec<u64>>,
    }

    impl PromptEmitter for FakeEmitter {
        fn asked(&self, prompt: &HostKeyPrompt) -> bool {
            self.asked.lock().unwrap().push(prompt.prompt_id);
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
}
