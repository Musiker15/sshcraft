//! A password kept in a password manager instead of SSHCraft's vault: a reference, fetched when
//! connecting by the manager's own program, started with an argument list. The reference is
//! data from sync or a share too, so it is checked strictly: it can name an item, never add an
//! option to the program.

use serde::{Deserialize, Serialize};

/// The longest reference taken.
pub const MAX_REFERENCE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manager {
    /// `pass-cli`, references `pass://<vault>/<item>/<field>`.
    ProtonPass,
    /// `op`, references `op://<vault>/<item>/<field>`.
    OnePassword,
    /// `bw`, the item's id; needs an unlocked session (`BW_SESSION`).
    Bitwarden,
    /// `keepassxc-cli`, references `<database file>#<entry path>`; the database password is
    /// asked once per run.
    KeePassXc,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProtonPass => "Proton Pass",
            Self::OnePassword => "1Password",
            Self::Bitwarden => "Bitwarden",
            Self::KeePassXc => "KeePassXC",
        }
    }

    /// The program it runs.
    pub fn program(self) -> &'static str {
        match self {
            Self::ProtonPass => "pass-cli",
            Self::OnePassword => "op",
            Self::Bitwarden => "bw",
            Self::KeePassXc => "keepassxc-cli",
        }
    }

    /// What a reference looks like, for the form.
    pub fn example(self) -> &'static str {
        match self {
            Self::ProtonPass => "pass://Servers/web/password",
            Self::OnePassword => "op://Servers/web/password",
            Self::Bitwarden => "8f3c…-item-id",
            Self::KeePassXc => "C:\\Users\\me\\servers.kdbx#Servers/web",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretRef {
    pub manager: Manager,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretRefError {
    #[error("the reference is empty or too long")]
    Length,
    #[error("the reference holds a control character or starts with '-'")]
    Unsafe,
    #[error("a {0} reference looks like {1}")]
    Shape(&'static str, &'static str),
}

impl SecretRef {
    /// Whether the reference is safe to hand to its program.
    pub fn check(&self) -> Result<(), SecretRefError> {
        let r = self.reference.as_str();
        if r.is_empty() || r.len() > MAX_REFERENCE {
            return Err(SecretRefError::Length);
        }
        if r.starts_with('-') || r.chars().any(char::is_control) {
            return Err(SecretRefError::Unsafe);
        }
        let shape = || SecretRefError::Shape(self.manager.label(), self.manager.example());
        let ok = match self.manager {
            Manager::ProtonPass => r.strip_prefix("pass://").is_some_and(|rest| rest.split('/').count() >= 2 && !rest.contains("//")),
            Manager::OnePassword => r.strip_prefix("op://").is_some_and(|rest| rest.split('/').count() >= 3 && !rest.contains("//")),
            Manager::Bitwarden => r.len() <= 64 && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            Manager::KeePassXc => {
                r.split_once('#').is_some_and(|(db, entry)| !db.is_empty() && !entry.is_empty() && !entry.starts_with('-') && !db.starts_with('-'))
            }
        };
        if ok { Ok(()) } else { Err(shape()) }
    }

    /// The arguments for the manager's program (KeePassXC reads the database password from
    /// standard input).
    pub fn arguments(&self) -> Result<Vec<String>, SecretRefError> {
        self.check()?;
        let r = self.reference.clone();
        Ok(match self.manager {
            Manager::ProtonPass => vec!["item".into(), "view".into(), r],
            Manager::OnePassword => vec!["read".into(), "--no-newline".into(), r],
            Manager::Bitwarden => vec!["get".into(), "password".into(), r],
            Manager::KeePassXc => {
                let (db, entry) = r.split_once('#').ok_or(SecretRefError::Unsafe)?;
                vec!["show".into(), "-q".into(), "-s".into(), "-a".into(), "Password".into(), db.to_owned(), entry.to_owned()]
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(manager: Manager, reference: &str) -> SecretRef {
        SecretRef { manager, reference: reference.into() }
    }

    #[test]
    fn a_reference_cannot_carry_an_option() {
        for bad in ["-oProxyCommand=x", "--session=evil", "op://a/b/c\n--x", ""] {
            assert!(r(Manager::OnePassword, bad).check().is_err(), "{bad:?}");
            assert!(r(Manager::Bitwarden, bad).check().is_err(), "{bad:?}");
        }
        assert!(r(Manager::KeePassXc, "db.kdbx#-x").check().is_err());
        assert!(r(Manager::KeePassXc, "-db.kdbx#x").check().is_err());
        // Counter-check: the usual shapes pass.
        assert!(r(Manager::OnePassword, "op://Servers/web/password").check().is_ok());
        assert!(r(Manager::ProtonPass, "pass://Servers/web/password").check().is_ok());
        assert!(r(Manager::Bitwarden, "8f3c1a2b-0000-4000-8000-123456789abc").check().is_ok());
        assert!(r(Manager::KeePassXc, "C:\\Users\\me\\servers.kdbx#Servers/web").check().is_ok());
    }

    #[test]
    fn each_manager_has_its_shape() {
        assert!(matches!(r(Manager::OnePassword, "pass://a/b/c").check(), Err(SecretRefError::Shape(..))));
        assert!(matches!(r(Manager::ProtonPass, "pass://onlyvault").check(), Err(SecretRefError::Shape(..))));
        assert!(matches!(r(Manager::Bitwarden, "has space").check(), Err(SecretRefError::Shape(..))));
    }

    #[test]
    fn the_reference_is_one_argument() {
        assert_eq!(r(Manager::OnePassword, "op://a b/c;d/e").arguments().unwrap(), vec!["read", "--no-newline", "op://a b/c;d/e"]);
        assert_eq!(
            r(Manager::KeePassXc, "/home/me/s.kdbx#Servers/web").arguments().unwrap(),
            vec!["show", "-q", "-s", "-a", "Password", "/home/me/s.kdbx", "Servers/web"]
        );
    }
}
