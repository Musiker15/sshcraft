//! The on-screen keyboard through `de.musiker15.sshcraft.MainActivity` (in `java/`): the text
//! it commits (typed, swiped, pasted from its clipboard) and showing or hiding it for the input
//! view that has the text connection.

// The JavaVM and the activity are only reachable as raw pointers from android-activity. This
// module is one of the two places in this crate that may use unsafe (AGENTS.md, "Never
// crash"); every block states why it is sound.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

use jni::objects::{JObject, JString, JValue};
use jni::{Env, JavaVM, jni_sig, jni_str};

/// At most this many commits are taken in one frame.
const MAX_PER_FRAME: usize = 64;

/// The activity, from `AndroidApp::activity_as_ptr`. Not the context in ndk-context: that is the
/// application, which has none of MainActivity's methods.
static ACTIVITY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Keeps the activity for the calls below; android_main gives it before the first frame.
pub fn set_activity(activity: *mut c_void) {
    ACTIVITY.store(activity, Ordering::Release);
}

fn vm() -> JavaVM {
    let context = ndk_context::android_context();
    // SAFETY: android-activity stores the process's JavaVM in ndk-context before it calls
    // android_main, and a JavaVM lives as long as the process.
    unsafe { JavaVM::from_raw(context.vm().cast()) }
}

/// Runs `f` with the activity on a thread attached to the VM.
pub(crate) fn with_activity<T>(f: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<T>) -> Result<T, String> {
    let raw = ACTIVITY.load(Ordering::Acquire);
    if raw.is_null() {
        return Err("no activity yet".into());
    }
    vm().attach_current_thread(|env| -> jni::errors::Result<T> {
        // SAFETY: android-activity keeps this reference to the activity valid while the activity
        // exists, and the process ends with it (android_main). The wrapper never deletes it.
        let activity = unsafe { JObject::from_raw(env, raw.cast()) };
        f(env, &activity)
    })
    .map_err(|e| e.to_string())
}

/// What the keyboard committed since the last frame, oldest first.
pub fn take_text() -> Result<Vec<String>, String> {
    with_activity(|env, activity| {
        let class = env.get_object_class(activity)?;
        let mut out = Vec::new();
        while out.len() < MAX_PER_FRAME {
            let text = env.call_static_method(&class, jni_str!("takeText"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
            if text.is_null() {
                break;
            }
            let text = env.cast_local::<JString>(text)?;
            out.push(text.try_to_string(env)?);
        }
        Ok(out)
    })
    .map_err(|e| format!("takeText: {e}"))
}

/// What the system covers of the window, packed as `MainActivity.covered` packs it.
pub fn covered() -> Result<u64, String> {
    with_activity(|env, activity| env.call_method(activity, jni_str!("covered"), jni_sig!("()J"), &[])?.j())
        .map(|packed| packed as u64)
        .map_err(|e| format!("covered: {e}"))
}

/// Shows the keyboard for the text connection, or hides it.
pub fn show(visible: bool) -> Result<(), String> {
    with_activity(|env, activity| {
        env.call_method(activity, jni_str!("keyboard"), jni_sig!("(Z)V"), &[JValue::Bool(visible)])?;
        Ok(())
    })
    .map_err(|e| format!("keyboard: {e}"))
}
