//! The credential references an item carries (key, password, identity, proxy password) and the
//! two rules for them: a record carries a reference only when what it points to goes into the
//! same area at the same server, and a record without a reference keeps the local one when the
//! local one points somewhere the record could not have carried it.

use serde::{Deserialize, Serialize};
use sshcraft_core::model::{ConnectionSettings, IdentityId, KeyRef, PasswordId, Proxy, ServerId};
use sshcraft_core::store::{ItemKind, Store};
use uuid::Uuid;

use crate::SyncError;
use crate::items::ItemBody;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRefs {
    #[serde(default)]
    pub key: Option<KeyRef>,
    #[serde(default)]
    pub password: Option<PasswordId>,
    #[serde(default)]
    pub identity: Option<IdentityId>,
    #[serde(default)]
    pub proxy_password: Option<PasswordId>,
}

impl CredentialRefs {
    pub fn of_settings(settings: &ConnectionSettings) -> Self {
        Self {
            key: settings.key.clone(),
            password: settings.password,
            identity: settings.identity,
            proxy_password: match &settings.proxy {
                Some(Proxy::Http(e) | Proxy::Socks5(e)) => e.password,
                _ => None,
            },
        }
    }

    /// Writes the references into settings; the proxy password only where there is a proxy.
    pub fn apply_to_settings(&self, settings: &mut ConnectionSettings) {
        settings.key = self.key.clone();
        settings.password = self.password;
        settings.identity = self.identity;
        if let Some(Proxy::Http(e) | Proxy::Socks5(e)) = &mut settings.proxy {
            e.password = self.proxy_password;
        }
    }
}

/// The references of a host, group or identity record.
pub(crate) fn refs_of(body: &ItemBody) -> Option<CredentialRefs> {
    match body {
        ItemBody::Host { host } => Some(CredentialRefs::of_settings(&host.settings)),
        ItemBody::Group { group } => Some(CredentialRefs::of_settings(&group.defaults)),
        ItemBody::Identity { identity } => Some(CredentialRefs {
            key: identity.key.clone(),
            password: identity.password,
            ..Default::default()
        }),
        _ => None,
    }
}

pub(crate) fn set_refs(body: &mut ItemBody, refs: &CredentialRefs) {
    match body {
        ItemBody::Host { host } => refs.apply_to_settings(&mut host.settings),
        ItemBody::Group { group } => refs.apply_to_settings(&mut group.defaults),
        ItemBody::Identity { identity } => {
            identity.key = refs.key.clone();
            identity.password = refs.password;
        }
        _ => {}
    }
}

/// Whether the item `id` of `kind` goes into `target` at `server`.
fn goes_with(
    store: &Store,
    server: ServerId,
    kind: ItemKind,
    id: &str,
    target: Uuid,
    personal: Uuid,
) -> Result<bool, SyncError> {
    Ok(store.location_of(kind, id)? == Some(server)
        && store.area_for(server, kind, id, personal)? == target)
}

/// Drops every reference whose item does not go into `target` at `server`. Agent keys only go
/// into the personal area.
pub(crate) fn strip_unsynced(
    store: &Store,
    server: ServerId,
    refs: &mut CredentialRefs,
    target: Uuid,
    personal: Uuid,
) -> Result<(), SyncError> {
    let keep_key = match &refs.key {
        Some(KeyRef::Stored { id }) => goes_with(
            store,
            server,
            ItemKind::Key,
            &id.0.to_string(),
            target,
            personal,
        )?,
        Some(KeyRef::Agent { .. }) => target == personal,
        None => true,
    };
    if !keep_key {
        refs.key = None;
    }
    for password in [&mut refs.password, &mut refs.proxy_password] {
        if let Some(id) = *password
            && !goes_with(
                store,
                server,
                ItemKind::Password,
                &id.0.to_string(),
                target,
                personal,
            )?
        {
            *password = None;
        }
    }
    if let Some(id) = refs.identity
        && !goes_with(
            store,
            server,
            ItemKind::Identity,
            &id.0.to_string(),
            target,
            personal,
        )?
    {
        refs.identity = None;
    }
    Ok(())
}

/// Whether a local reference to `id` points where a record of `area` could not carry it:
/// elsewhere than `server`, or (for a record of a known area) in another area.
fn foreign(
    store: &Store,
    server: ServerId,
    kind: ItemKind,
    id: &str,
    area: Option<Uuid>,
    personal: Option<Uuid>,
) -> Result<bool, SyncError> {
    let elsewhere = store.location_of(kind, id)? != Some(server);
    let other_area = match area {
        Some(area) => store.area_for(server, kind, id, personal.unwrap_or_default())? != area,
        None => false,
    };
    Ok(elsewhere || other_area)
}

/// Fills references a record does not carry with the local ones it could not have carried; a
/// missing reference the record could have carried means "removed".
pub(crate) fn keep_local(
    store: &Store,
    server: ServerId,
    local: &CredentialRefs,
    incoming: &mut CredentialRefs,
    area: Option<Uuid>,
    personal: Option<Uuid>,
) -> Result<(), SyncError> {
    let shared_area = area.is_some() && area != personal;
    if incoming.key.is_none() {
        incoming.key = match &local.key {
            Some(KeyRef::Stored { id }) => foreign(
                store,
                server,
                ItemKind::Key,
                &id.0.to_string(),
                area,
                personal,
            )?
            .then_some(KeyRef::Stored { id: *id }),
            Some(agent @ KeyRef::Agent { .. }) if shared_area => Some(agent.clone()),
            _ => None,
        };
    }
    for (incoming, local) in [
        (&mut incoming.password, local.password),
        (&mut incoming.proxy_password, local.proxy_password),
    ] {
        if incoming.is_none()
            && let Some(id) = local
            && foreign(
                store,
                server,
                ItemKind::Password,
                &id.0.to_string(),
                area,
                personal,
            )?
        {
            *incoming = Some(id);
        }
    }
    if incoming.identity.is_none()
        && let Some(id) = local.identity
        && foreign(
            store,
            server,
            ItemKind::Identity,
            &id.0.to_string(),
            area,
            personal,
        )?
    {
        incoming.identity = Some(id);
    }
    Ok(())
}
