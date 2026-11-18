//! The system's file picker for the app's file dialogs (`file_dialogs::picked`): `MainActivity`
//! opens it and answers with a path to the picked file that stays valid for a while
//! (`/proc/self/fd/<n>`), so imports, exports and backups read and write as on the desktop.

use std::path::PathBuf;

use jni::objects::{JString, JValue};
use jni::{jni_sig, jni_str};
use sshcraft_ui_egui::file_dialogs::picked::{FileDialog, Kind, Picker, mime_for, parse_answer};

/// At most this many answers are taken in one frame.
const MAX_PER_FRAME: usize = 16;

pub struct SystemPicker;

impl Picker for SystemPicker {
    fn ask(&self, id: u64, dialog: &FileDialog) -> Result<(), String> {
        let save = dialog.kind == Kind::Save;
        let name = dialog.file_name.clone().unwrap_or_default();
        let mime = mime_for(&dialog.extensions);
        crate::keyboard::with_activity(|env, activity| {
            let name = env.new_string(&name)?;
            let mime = env.new_string(mime)?;
            let id = i64::try_from(id).unwrap_or(i64::MAX);
            env.call_method(
                activity,
                jni_str!("pick"),
                jni_sig!("(JZLjava/lang/String;Ljava/lang/String;)V"),
                &[JValue::Long(id), JValue::Bool(save), JValue::Object(&name), JValue::Object(&mime)],
            )?;
            Ok(())
        })
        .map_err(|e| format!("pick: {e}"))
    }

    fn answers(&self) -> Vec<(u64, Option<PathBuf>)> {
        let taken = crate::keyboard::with_activity(|env, activity| {
            let class = env.get_object_class(activity)?;
            let mut out = Vec::new();
            while out.len() < MAX_PER_FRAME {
                let line = env.call_static_method(&class, jni_str!("takePicked"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
                if line.is_null() {
                    break;
                }
                let line = env.cast_local::<JString>(line)?;
                out.push(line.try_to_string(env)?);
            }
            Ok(out)
        });
        match taken {
            Ok(lines) => lines.iter().filter_map(|line| parse_answer(line)).collect(),
            Err(e) => {
                log::warn!("takePicked: {e}");
                Vec::new()
            }
        }
    }
}
