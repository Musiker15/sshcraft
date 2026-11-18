//! Open sessions in the background, and notifications. Android stops a backgrounded app's
//! network, and with it every SSH session; a foreground service with its notification keeps the
//! app running while sessions are open (`SessionService`). The app's own messages (a long command
//! ended, a server's message) go out as notifications too. Both through `MainActivity`, which
//! hands them to Android; what they say and when comes from here.

use std::sync::atomic::{AtomicI32, Ordering};

use jni::objects::JValue;
use jni::{jni_sig, jni_str};
use sshcraft_ui_egui::i18n::tr;

/// The ids of message notifications; 1 is the service's own.
static NEXT_ID: AtomicI32 = AtomicI32::new(2);

/// Tells the activity how many sessions are open: it starts, updates or stops the service.
pub fn sessions(count: usize) -> Result<(), String> {
    let text = sshcraft_ui_egui::notify::sessions_notice(count).unwrap_or_default();
    crate::keyboard::with_activity(|env, activity| {
        let text = env.new_string(&text)?;
        let channel = env.new_string(tr("Open sessions"))?;
        let count = i32::try_from(count).unwrap_or(i32::MAX);
        env.call_method(
            activity,
            jni_str!("sessions"),
            jni_sig!("(ILjava/lang/String;Ljava/lang/String;)V"),
            &[JValue::Int(count), JValue::Object(&text), JValue::Object(&channel)],
        )?;
        Ok(())
    })
}

/// The app's notifications as Android notifications.
pub struct Notices;

impl sshcraft_ui_egui::notify::Notifier for Notices {
    fn notify(&self, title: &str, body: &str) {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(2);
        let sent = crate::keyboard::with_activity(|env, activity| {
            let title = env.new_string(title)?;
            let body = env.new_string(body)?;
            let channel = env.new_string(tr("Messages"))?;
            env.call_method(
                activity,
                jni_str!("notify"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V"),
                &[JValue::Object(&title), JValue::Object(&body), JValue::Object(&channel), JValue::Int(id)],
            )?;
            Ok(())
        });
        if let Err(e) = sent {
            log::warn!("a notification could not be shown: {e}");
        }
    }
}
