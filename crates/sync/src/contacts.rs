//! The accounts this device shares with: found by e-mail, trusted by their account key.
//!
//! The e-mail is only a search term (design, section 7). The first key seen for an e-mail is
//! remembered; the user compares its safety number with the other person over another channel
//! and confirms it. A different key for the same e-mail later is reported and never replaces
//! the known one on its own.

use crate::SyncError;
use crate::engine::{Engine, lock};
use serde::Serialize;
use sshcraft_model::model::ServerId;
use sshcraft_proto::identity::{AccountPublic, safety_number};
use sshcraft_store::store::{Contact, Store};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum KnownState {
    /// Never seen before on this device.
    New,
    /// Seen before and its safety number confirmed.
    Confirmed,
    /// Seen before, not confirmed yet.
    Unconfirmed,
    /// The server now names another account for this e-mail than the one known.
    Changed { previous_safety_number: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recipient {
    pub account: AccountPublic,
    pub name: String,
    pub email: String,
    pub safety_number: String,
    pub known: KnownState,
}

/// E-mails compare trimmed and in lower case.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Looks an account up by e-mail and compares it with what this device knows.
pub async fn find(
    engine: &Engine,
    store: &Mutex<Store>,
    email: &str,
) -> Result<Recipient, SyncError> {
    let email = normalize_email(email);
    let lookup = engine.client().lookup(&email).await;
    let (account, name) = engine.revoked_check(store, lookup)?;
    if account == engine.account {
        return Err(SyncError::NotAllowed);
    }
    let guard = lock(store)?;
    let known = match guard.contact(engine.server, &email)? {
        None => {
            guard.put_contact(
                engine.server,
                &Contact {
                    email: email.clone(),
                    account: account.0,
                    name: name.clone(),
                    first_seen: now_secs(),
                    confirmed: false,
                },
            )?;
            KnownState::New
        }
        Some(contact) if contact.account == account.0 => {
            guard.put_contact(
                engine.server,
                &Contact {
                    name: name.clone(),
                    ..contact.clone()
                },
            )?;
            if contact.confirmed {
                KnownState::Confirmed
            } else {
                KnownState::Unconfirmed
            }
        }
        // Kept as it is: only the user's confirmation replaces a known key.
        Some(contact) => KnownState::Changed {
            previous_safety_number: safety_number(&AccountPublic(contact.account)),
        },
    };
    Ok(Recipient {
        safety_number: safety_number(&account),
        account,
        name,
        email,
        known,
    })
}

/// The user compared the safety number of `account` (for a changed key: accepted the new one).
pub fn confirm(
    store: &Mutex<Store>,
    server: ServerId,
    email: &str,
    account: &AccountPublic,
) -> Result<(), SyncError> {
    lock(store)?.confirm_contact(server, &normalize_email(email), &account.0)?;
    Ok(())
}

/// Sharing with `account` needs a confirmed contact with exactly this key.
pub fn require_confirmed(
    store: &Mutex<Store>,
    server: ServerId,
    account: &AccountPublic,
) -> Result<(), SyncError> {
    match lock(store)?.contact_by_account(server, &account.0)? {
        Some(contact) if contact.confirmed => Ok(()),
        _ => Err(SyncError::ContactUnconfirmed),
    }
}
