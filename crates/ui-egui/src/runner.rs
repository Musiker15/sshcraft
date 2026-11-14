//! Runs engine commands without blocking a frame: each runs as a task on the engine's runtime,
//! and its result comes back through a channel; the frame loop applies it to the app with the
//! continuation given when it was started.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;
use sshcraft_engine::{Caller, EngineError, Session};

use crate::app::SshcraftApp;

/// What happens with a command's result, on the frame loop's thread.
pub type Then = Box<dyn FnOnce(&mut SshcraftApp, Result<Value, EngineError>) + Send>;

pub struct Runner {
    session: Session,
    sender: Sender<(Then, Result<Value, EngineError>)>,
    receiver: Mutex<Receiver<(Then, Result<Value, EngineError>)>>,
    busy: Arc<AtomicUsize>,
    repaint: egui::Context,
}

impl Runner {
    pub fn new(session: Session, repaint: egui::Context) -> Self {
        let (sender, receiver) = channel();
        Self { session, sender, receiver: Mutex::new(receiver), busy: Arc::new(AtomicUsize::new(0)), repaint }
    }

    /// Runs command `id` for the user; `then` gets the result in a later frame.
    pub fn run(&self, id: &str, params: Value, then: Then) {
        let session = self.session.clone();
        let id = id.to_string();
        self.run_future(async move { session.execute(Caller::User, &id, params).await }, then);
    }

    /// Runs `work` on the engine's runtime; `then` gets its result in a later frame.
    pub fn run_future<F>(&self, work: F, then: Then)
    where
        F: Future<Output = Result<Value, EngineError>> + Send + 'static,
    {
        let sender = self.sender.clone();
        let busy = self.busy.clone();
        let repaint = self.repaint.clone();
        busy.fetch_add(1, Ordering::SeqCst);
        self.session.runtime().spawn(async move {
            let result = work.await;
            // The app may be gone (window closed): then nobody waits for the result.
            let _ = sender.send((then, result));
            busy.fetch_sub(1, Ordering::SeqCst);
            repaint.request_repaint();
        });
    }

    /// Commands started and not yet back.
    pub fn busy(&self) -> usize {
        self.busy.load(Ordering::SeqCst)
    }

    /// The results that came back since the last call.
    pub fn take_done(&self) -> Vec<(Then, Result<Value, EngineError>)> {
        let receiver = self.receiver.lock().unwrap_or_else(PoisonError::into_inner);
        receiver.try_iter().collect()
    }
}
