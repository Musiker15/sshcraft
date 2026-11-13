//! Workspace templates: which sessions a workspace opens, how they are laid out, where they
//! start and what they run. Kept on this device only (one setting), never synced.

use serde::{Deserialize, Serialize};
use sshcraft_core::model::HostId;
use sshcraft_core::store::{Store, StoreError};
use uuid::Uuid;

const KEY: &str = "workspace.templates";
pub const MAX_TEMPLATES: usize = 100;
pub const MAX_PANES: usize = 16;
pub const MAX_TEXT: usize = 1000;
pub const MAX_NAME: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    Split,
    Focus,
}

/// What a pane connects to; the same cases as `SessionTarget`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateTarget {
    Host {
        host_id: HostId,
    },
    #[cfg(desktop)]
    Local {
        shell: sshcraft_local::pty::ShellCommand,
    },
    #[cfg(desktop)]
    Serial {
        settings: sshcraft_local::serial::SerialSettings,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatePane {
    pub target: TemplateTarget,
    pub title: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub layout: Layout,
    pub col_sizes: Vec<f32>,
    pub row_sizes: Vec<f32>,
    pub panes: Vec<TemplatePane>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("invalid template: {0}")]
    Invalid(&'static str),
    #[error("too many templates")]
    TooMany,
    #[error("store: {0}")]
    Store(String),
}

impl From<StoreError> for TemplateError {
    fn from(e: StoreError) -> Self {
        Self::Store(e.to_string())
    }
}

fn text_ok(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn sizes_ok(sizes: &[f32]) -> bool {
    sizes.len() <= MAX_PANES && sizes.iter().all(|s| s.is_finite() && *s > 0.0)
}

/// Checks a template against the limits above.
pub fn validate(t: &Template) -> Result<(), TemplateError> {
    let name = t.name.trim();
    if name.is_empty() || !text_ok(name, MAX_NAME) {
        return Err(TemplateError::Invalid("name"));
    }
    if t.panes.is_empty() || t.panes.len() > MAX_PANES {
        return Err(TemplateError::Invalid("panes"));
    }
    if !sizes_ok(&t.col_sizes) || !sizes_ok(&t.row_sizes) {
        return Err(TemplateError::Invalid("sizes"));
    }
    for pane in &t.panes {
        let fields = [Some(&pane.title), pane.cwd.as_ref(), pane.command.as_ref()];
        if !fields.into_iter().flatten().all(|f| text_ok(f, MAX_TEXT)) {
            return Err(TemplateError::Invalid("pane"));
        }
    }
    Ok(())
}

/// The stored templates; a damaged entry is left out, damaged storage gives none.
pub fn list(store: &Store) -> Vec<Template> {
    let Some(serde_json::Value::Array(items)) = store
        .get_setting(KEY)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
    else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<Template>(item).ok())
        .filter(|t| validate(t).is_ok())
        .take(MAX_TEMPLATES)
        .collect()
}

fn write(store: &Store, templates: &[Template]) -> Result<(), TemplateError> {
    let json = serde_json::to_string(templates).map_err(|e| TemplateError::Store(e.to_string()))?;
    store.put_setting(KEY, &json)?;
    Ok(())
}

/// Stores `template`; one with the same id is replaced.
pub fn save(store: &Store, mut template: Template) -> Result<Template, TemplateError> {
    template.name = template.name.trim().to_owned();
    validate(&template)?;
    let mut templates = list(store);
    match templates.iter().position(|t| t.id == template.id) {
        Some(index) => templates[index] = template.clone(),
        None if templates.len() >= MAX_TEMPLATES => return Err(TemplateError::TooMany),
        None => templates.push(template.clone()),
    }
    write(store, &templates)?;
    Ok(template)
}

pub fn delete(store: &Store, id: Uuid) -> Result<(), TemplateError> {
    let mut templates = list(store);
    templates.retain(|t| t.id != id);
    write(store, &templates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(n: usize) -> TemplatePane {
        TemplatePane {
            target: TemplateTarget::Host {
                host_id: HostId::new(),
            },
            title: format!("pane {n}"),
            cwd: Some("/srv/app".into()),
            command: Some("htop".into()),
        }
    }

    fn template(name: &str, panes: usize) -> Template {
        Template {
            id: Uuid::new_v4(),
            name: name.into(),
            layout: Layout::Split,
            col_sizes: vec![1.0, 1.0],
            row_sizes: vec![1.0],
            panes: (0..panes).map(pane).collect(),
        }
    }

    fn invalid(t: Template) -> bool {
        matches!(validate(&t), Err(TemplateError::Invalid(_)))
    }

    #[test]
    fn sixteen_panes_and_umlauts_are_fine() {
        assert_eq!(validate(&template("Größe und Übersicht", 16)), Ok(()));
    }

    #[test]
    fn bad_names_are_refused() {
        assert!(invalid(template("", 2)));
        assert!(invalid(template("   ", 2)));
        assert!(invalid(template(&"x".repeat(101), 2)));
        assert!(invalid(template("a\nb", 2)));
    }

    #[test]
    fn pane_count_is_limited() {
        assert!(invalid(template("w", 0)));
        assert!(invalid(template("w", 17)));
    }

    #[test]
    fn control_characters_and_long_text_are_refused() {
        let mut t = template("w", 2);
        t.panes[0].command = Some("ls\nrm -rf /".into());
        assert!(invalid(t));
        let mut t = template("w", 2);
        t.panes[1].command = Some("echo \u{1b}[31m".into());
        assert!(invalid(t));
        let mut t = template("w", 2);
        t.panes[0].cwd = Some("x".repeat(1001));
        assert!(invalid(t));
        let mut t = template("w", 2);
        t.panes[0].cwd = Some("x".repeat(1000));
        assert_eq!(validate(&t), Ok(()));
    }

    #[test]
    fn sizes_must_be_positive_numbers() {
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut t = template("w", 2);
            t.col_sizes = vec![1.0, bad];
            assert!(invalid(t), "{bad}");
        }
    }

    #[test]
    fn save_list_delete_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let a = save(&store, template("A", 2)).unwrap();
        let b = save(&store, template("B", 3)).unwrap();
        assert_eq!(list(&store), vec![a.clone(), b.clone()]);
        delete(&store, a.id).unwrap();
        assert_eq!(list(&store), vec![b]);
    }

    #[test]
    fn save_same_id_replaces() {
        let store = Store::open_in_memory().unwrap();
        let mut a = save(&store, template("A", 2)).unwrap();
        a.name = "A2".into();
        save(&store, a.clone()).unwrap();
        assert_eq!(list(&store), vec![a]);
    }

    #[test]
    fn hundred_and_first_is_refused() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..MAX_TEMPLATES {
            save(&store, template(&format!("t{i}"), 1)).unwrap();
        }
        assert_eq!(
            save(&store, template("one more", 1)),
            Err(TemplateError::TooMany)
        );
        assert_eq!(list(&store).len(), MAX_TEMPLATES);
    }

    #[test]
    fn broken_entry_is_skipped() {
        let store = Store::open_in_memory().unwrap();
        let good = template("good", 2);
        let bad = template("bad", 40);
        let json = serde_json::to_string(&vec![good.clone(), bad]).unwrap();
        store.put_setting(KEY, &json).unwrap();
        assert_eq!(list(&store), vec![good]);
    }

    #[test]
    fn garbled_json_gives_empty_list() {
        let store = Store::open_in_memory().unwrap();
        store.put_setting(KEY, "{not json").unwrap();
        assert!(list(&store).is_empty());
        store.put_setting(KEY, r#"{"a":1}"#).unwrap();
        assert!(list(&store).is_empty());
    }
}
