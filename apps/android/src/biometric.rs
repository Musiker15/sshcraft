//! The user's presence on Android: the system's biometric prompt (fingerprint, face, or the
//! device's screen lock) through `de.musiker15.sshcraft.Biometric` in classes.dex. It guards the
//! app lock and the first use of a key or password after it, as Windows Hello does on Windows.

// The JavaVM and the activity are only reachable as the raw pointers android-activity put into
// ndk-context. This module is the one place in this crate that may use unsafe (AGENTS.md, "Never
// crash"); every block states why it is sound.
#![allow(unsafe_code)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use jni::objects::{Global, JClass, JObject, JValue};
use jni::{Env, JavaVM, jni_sig, jni_str};
use sshcraft_fido::presence::{Presence, UserPresence};

use crate::Answer;

/// How often an open prompt is asked for its answer.
const POLL: Duration = Duration::from_millis(100);

/// The class, loaded once through the activity's class loader: on a thread the app attached
/// itself, `FindClass` sees only the system's classes.
static CLASS: OnceLock<Global<JClass<'static>>> = OnceLock::new();

/// Tokens tell the answers of prompts apart.
static NEXT: AtomicI64 = AtomicI64::new(1);

fn vm() -> JavaVM {
    let context = ndk_context::android_context();
    // SAFETY: android-activity stores the process's JavaVM in ndk-context before it calls
    // android_main, and a JavaVM lives as long as the process.
    unsafe { JavaVM::from_raw(context.vm().cast()) }
}

/// Runs `f` with the activity and the Biometric class on a thread attached to the VM.
fn with_class<T>(f: impl FnOnce(&mut Env, &JObject, &JClass) -> jni::errors::Result<T>) -> Result<T, String> {
    vm().attach_current_thread(|env| -> jni::errors::Result<T> {
        let raw = ndk_context::android_context().context();
        // SAFETY: the context in ndk-context is a global reference to the NativeActivity that
        // android-activity holds while the activity exists, which is as long as this app runs.
        let activity = unsafe { JObject::from_raw(env, raw.cast()) };
        let class = match CLASS.get() {
            Some(class) => class,
            None => {
                let loader = env.call_method(&activity, jni_str!("getClassLoader"), jni_sig!("()Ljava/lang/ClassLoader;"), &[])?.l()?;
                let name = env.new_string("de.musiker15.sshcraft.Biometric")?;
                let found = env
                    .call_method(&loader, jni_str!("loadClass"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"), &[JValue::Object(&name)])?
                    .l()?;
                let class = env.cast_local::<JClass>(found)?;
                let global = env.new_global_ref(&class)?;
                CLASS.get_or_init(|| global)
            }
        };
        f(env, &activity, class)
    })
    .map_err(|e| e.to_string())
}

/// Whether the device can confirm the user (a strong biometric or a screen lock is set up).
pub fn available() -> bool {
    with_class(|env, activity, class| {
        env.call_static_method(class, jni_str!("available"), jni_sig!("(Landroid/app/Activity;)Z"), &[JValue::Object(activity)])?.z()
    })
    .unwrap_or(false)
}

fn ask(title: &str, subtitle: &str, token: i64) -> Result<(), String> {
    with_class(|env, activity, class| {
        let title = env.new_string(title)?;
        let subtitle = env.new_string(subtitle)?;
        env.call_static_method(
            class,
            jni_str!("ask"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;J)V"),
            &[JValue::Object(activity), JValue::Object(&title), JValue::Object(&subtitle), JValue::Long(token)],
        )?;
        Ok(())
    })
}

fn poll(token: i64) -> Answer {
    with_class(|env, _, class| env.call_static_method(class, jni_str!("poll"), jni_sig!("(J)I"), &[JValue::Long(token)])?.i())
        .map_or(Answer::Declined, crate::answer)
}

/// The biometric prompt as the engine's user presence.
pub struct Biometric;

#[async_trait::async_trait]
impl UserPresence for Biometric {
    async fn verify(&self, reason: &str) -> Presence {
        // No biometric and no screen lock: nothing can confirm the user, as without Windows Hello.
        if !available() {
            return Presence::Unavailable;
        }
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        if ask("SSHCraft", reason, token).is_err() {
            return Presence::Declined;
        }
        loop {
            tokio::time::sleep(POLL).await;
            match poll(token) {
                Answer::Pending => {}
                Answer::Verified => return Presence::Verified,
                Answer::Declined => return Presence::Declined,
            }
        }
    }
}
