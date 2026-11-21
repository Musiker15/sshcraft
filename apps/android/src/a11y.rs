//! Screen readers (TalkBack) and other accessibility services through AccessKit: egui's tree of
//! each frame goes to the view of NativeActivity, and what the screen reader asks for (a tap, a
//! focus, a scroll) comes back as input of the next frame.
//!
//! eframe connects AccessKit on Android only with GameActivity, because it looks for that
//! activity's view. The adapter itself only needs a view to hang on, so this module gives it the
//! one NativeActivity draws into (`MainActivity.screen`), and the app stays on NativeActivity.

// The JavaVM and the activity are only reachable as raw pointers from android-activity. This
// module is one of the places in this crate that may use unsafe (AGENTS.md, "Never crash");
// every block states why it is sound.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, PoisonError};

use accesskit_android::InjectingAdapter;
use accesskit_android::jni::JavaVM;
use accesskit_android::jni::objects::JObject;
use egui::accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};

/// Asked by the screen reader for the first tree: egui builds it from the next frame on.
struct Activation {
    ctx: egui::Context,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Not built while nothing asks for it; once asked, every frame carries the whole tree.
        self.ctx.enable_accesskit();
        self.ctx.request_repaint();
        None
    }
}

/// What the screen reader asks for, handed to the frame (it runs on Android's UI thread).
struct Actions {
    sender: Sender<ActionRequest>,
    ctx: egui::Context,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // A frame that is gone has nobody to tell.
        if self.sender.send(request).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

/// Hands each frame's tree to the adapter. The tree is there only while a service asked for it.
struct TreeToAndroid {
    adapter: Mutex<InjectingAdapter>,
}

impl egui::Plugin for TreeToAndroid {
    fn debug_name(&self) -> &'static str {
        "sshcraft_android_accessibility"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        if let Some(update) = &output.platform_output.accesskit_update {
            let mut adapter = self.adapter.lock().unwrap_or_else(PoisonError::into_inner);
            adapter.update_if_active(|| update.clone());
        }
    }
}

/// The requests of the screen reader for the frame.
pub struct ScreenReader {
    requests: Receiver<ActionRequest>,
}

impl ScreenReader {
    /// Hangs AccessKit on the view of `activity` and the tree of `ctx` on it.
    pub fn attach(ctx: &egui::Context, activity: *mut c_void) -> Result<Self, String> {
        if activity.is_null() {
            return Err("no activity".into());
        }
        let context = ndk_context::android_context();
        // SAFETY: android-activity stores the process's JavaVM in ndk-context before it calls
        // android_main, and a JavaVM lives as long as the process.
        let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        // SAFETY: android-activity keeps this reference to the activity valid while the activity
        // exists, and the process ends with it (android_main). JObject never deletes it.
        let activity = unsafe { JObject::from_raw(activity.cast()) };
        let view = env.call_method(&activity, "screen", "()Landroid/view/View;", &[]).and_then(|v| v.l()).map_err(|e| format!("screen: {e}"))?;
        if view.is_null() {
            return Err("screen: no view".into());
        }
        let (sender, requests) = channel();
        let adapter = InjectingAdapter::new(&mut env, &view, Activation { ctx: ctx.clone() }, Actions { sender, ctx: ctx.clone() });
        ctx.add_plugin(TreeToAndroid { adapter: Mutex::new(adapter) });
        Ok(Self { requests })
    }

    /// Puts what the screen reader asked for since the last frame into its input.
    pub fn feed(&self, raw_input: &mut egui::RawInput) {
        crate::screen_reader_input(self.requests.try_iter(), &mut raw_input.events);
    }
}
