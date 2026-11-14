//! Keys in the vault, security keys (FIDO2, Windows Hello) and keys from Proton Pass. Answers
//! carry only public facts ([`KeyInfo`]), never key material or a passphrase.

use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm};
use sshcraft_model::keytypes::KeySpec;
use sshcraft_model::model::{KeyId, ServerId};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_store::vault::{self, KeyInfo, Vault, VaultError};
use zeroize::Zeroizing;

use super::{CommandError, CommandResult, CommandSpec, Run, blocking_run, command, needs_vault, parse, query, sync_run, to_value};
use crate::error::ErrorCode;
use crate::passcli::{self, PassCliError, PassCliStatus};
use crate::sync::{note_change, now_ms, share_rights_for_key};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

/// A command that is disabled while the vault is locked.
fn vault_command(id: &'static str, label: &'static str, params: &'static str, run: Run) -> CommandSpec {
    CommandSpec { enabled: needs_vault, ..command(id, label, params, run) }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("key.list", "Keys", "none", run_list),
        vault_command(
            "key.import",
            "Import a key",
            "{ name, text: the private key, passphrase?, save_passphrase?: false keeps the key encrypted and asks for the passphrase on each use }",
            run_import,
        ),
        vault_command(
            "key.generate",
            "Generate a key",
            "{ name, spec: { type: ed25519 } | { type: ecdsa, curve: nistp256|nistp384|nistp521 } | { type: rsa, bits: 3072|4096 }, passphrase? }",
            run_generate,
        ),
        query("key.security_support", "Security key support", "none", run_security_support),
        vault_command(
            "key.create_security",
            "Create a security key",
            "{ name, kind: fido2|hello, algorithm: ecdsa_p256|ed25519, verify_user }",
            run_create_security,
        ),
        vault_command("key.attach_certificate", "Attach a certificate", "{ id, text: the OpenSSH certificate }", run_attach_certificate),
        vault_command("key.detach_certificate", "Detach the certificate", "{ id }", run_detach_certificate),
        vault_command("key.remember_passphrase", "Save the passphrase", "{ id, passphrase }", run_remember_passphrase),
        vault_command("key.protect", "Ask for the passphrase on each use", "{ id, passphrase }", run_protect),
        command("key.delete", "Delete a key", "{ id }", run_delete),
        query("key.passcli_status", "Proton Pass CLI status", "none", run_passcli_status),
        query("key.passcli_list", "Keys in Proton Pass", "none", run_passcli_list),
        vault_command("key.passcli_import", "Import a key from Proton Pass", "{ share_id, item_id, name }", run_passcli_import),
    ]
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&vault::list_keys(&ctx.session.store())?))
}

#[derive(Deserialize)]
struct ImportParams {
    name: String,
    text: Zeroizing<String>,
    #[serde(default)]
    passphrase: Option<Zeroizing<String>>,
    #[serde(default)]
    save_passphrase: Option<bool>,
}

/// Imports a key. With `save_passphrase == false` an encrypted key stays encrypted with its
/// passphrase, which is then asked on each use.
fn run_import(ctx: Ctx, params: Value) -> Out {
    blocking_run("key.import", move || {
        let ImportParams { name, text, passphrase, save_passphrase } = parse("key.import", params)?;
        let vault = ctx.session.vault()?;
        let passphrase = passphrase.filter(|p| !p.is_empty());
        let key = vault::parse_private_key(&text, passphrase.as_deref().map(String::as_str))?;
        let info = {
            let store = ctx.session.store();
            match passphrase {
                Some(p) if save_passphrase == Some(false) => vault.store_protected(&store, &name, &key, &p)?,
                _ => vault.store_key(&store, &name, &key)?,
            }
        };
        ctx.session.announce(&[]);
        to_value(&info)
    })
}

#[derive(Deserialize)]
struct GenerateParams {
    name: String,
    spec: KeySpec,
    #[serde(default)]
    passphrase: Option<Zeroizing<String>>,
}

fn run_generate(ctx: Ctx, params: Value) -> Out {
    blocking_run("key.generate", move || {
        let GenerateParams { name, spec, passphrase } = parse("key.generate", params)?;
        let vault = ctx.session.vault()?;
        let info = vault.generate(&ctx.session.store(), &name, &spec, passphrase.as_deref().map(String::as_str))?;
        ctx.session.announce(&[]);
        to_value(&info)
    })
}

#[derive(Serialize)]
struct SecuritySupport {
    fido2: bool,
    hello: bool,
}

/// Which security keys this device can create.
fn run_security_support(ctx: Ctx, _: Value) -> Out {
    let fido2 = ctx.session.security().is_some();
    async move {
        #[cfg(windows)]
        let hello = fido2 && tokio::task::spawn_blocking(sshcraft_fido::webauthn::platform_available).await.unwrap_or(false);
        #[cfg(not(windows))]
        let hello = false;
        to_value(&SecuritySupport { fido2, hello })
    }
    .boxed()
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SecurityKeyKind {
    /// A token you plug in (YubiKey and the like). Synced like any key.
    Fido2,
    /// Windows Hello: bound to this device, never synced.
    Hello,
}

#[derive(Deserialize)]
struct CreateSecurityParams {
    name: String,
    kind: SecurityKeyKind,
    algorithm: SkAlgorithm,
    verify_user: bool,
}

/// Creates a key on a FIDO2 token or in Windows Hello. Windows shows its own dialog.
fn run_create_security(ctx: Ctx, params: Value) -> Out {
    async move {
        let CreateSecurityParams { name, kind, algorithm, verify_user } = parse("key.create_security", params)?;
        let vault = ctx.session.vault()?;
        let auth = ctx.session.security().ok_or_else(|| CommandError::code(ErrorCode::NotSupported))?;
        let options = match kind {
            SecurityKeyKind::Fido2 => CreateOptions { algorithm, attachment: Attachment::CrossPlatform, verify_user, user_name: name.clone() },
            // Windows Hello holds only P-256 keys and always checks the user.
            SecurityKeyKind::Hello => {
                CreateOptions { algorithm: SkAlgorithm::EcdsaP256, attachment: Attachment::Platform, verify_user: true, user_name: name.clone() }
            }
        };
        let key = sshcraft_fido::create_key(auth.as_ref(), &options, &name).await?;
        let info = {
            let store = ctx.session.store();
            match kind {
                SecurityKeyKind::Fido2 => vault.store_key(&store, &name, &key)?,
                SecurityKeyKind::Hello => vault.store_device_key(&store, &name, &key)?,
            }
        };
        ctx.session.announce(&[]);
        to_value(&info)
    }
    .boxed()
}

/// A key changed: queue it for its server, if it has one.
pub(crate) fn key_changed(store: &Store, id: KeyId) -> CommandResult<Vec<ServerId>> {
    let key = id.0.to_string();
    let location = store.location_of(ItemKind::Key, &key)?;
    note_change(store, ItemKind::Key, &key, location, now_ms())
}

/// Runs `change` on a key this account may change, then queues it for sync.
fn change_key(session: &Session, id: KeyId, change: impl FnOnce(&Vault, &Store) -> Result<KeyInfo, VaultError>) -> CommandResult<Value> {
    let vault = session.vault()?;
    let (info, servers) = {
        let store = session.store();
        if let Some(server) = store.location_of(ItemKind::Key, &id.0.to_string())? {
            share_rights_for_key(&store, server, id)?;
        }
        let info = change(&vault, &store)?;
        (info, key_changed(&store, id)?)
    };
    session.sync().nudge(&servers);
    session.announce(&servers);
    to_value(&info)
}

#[derive(Deserialize)]
struct IdParams {
    id: KeyId,
}

#[derive(Deserialize)]
struct CertificateParams {
    id: KeyId,
    text: String,
}

#[derive(Deserialize)]
struct PassphraseParams {
    id: KeyId,
    passphrase: Zeroizing<String>,
}

fn run_attach_certificate(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let CertificateParams { id, text } = parse("key.attach_certificate", params)?;
        change_key(&ctx.session, id, |vault, store| vault.attach_certificate(store, id, &text))
    })
}

fn run_detach_certificate(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("key.detach_certificate", params)?;
        change_key(&ctx.session, id, |vault, store| vault.detach_certificate(store, id))
    })
}

/// Saves the passphrase from now on (the key is kept decrypted inside the vault). Runs off the
/// async threads: opening the key runs bcrypt, which can take a second.
fn run_remember_passphrase(ctx: Ctx, params: Value) -> Out {
    blocking_run("key.remember_passphrase", move || {
        let PassphraseParams { id, passphrase } = parse("key.remember_passphrase", params)?;
        change_key(&ctx.session, id, |vault, store| vault.remember_passphrase(store, id, &passphrase))
    })
}

/// Stops saving the passphrase: the key is kept encrypted with `passphrase`.
fn run_protect(ctx: Ctx, params: Value) -> Out {
    blocking_run("key.protect", move || {
        let PassphraseParams { id, passphrase } = parse("key.protect", params)?;
        change_key(&ctx.session, id, |vault, store| vault.protect_key(store, id, &passphrase))
    })
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("key.delete", params)?;
        let session = &ctx.session;
        let servers = {
            let store = session.store();
            let key = id.0.to_string();
            let before = store.location_of(ItemKind::Key, &key)?;
            if let Some(server) = before {
                share_rights_for_key(&store, server, id)?;
            }
            let servers = note_change(&store, ItemKind::Key, &key, before, now_ms())?;
            #[cfg(windows)]
            forget_hello_credential(session, &store, id);
            vault::delete_key(&store, id)?;
            servers
        };
        session.sync().nudge(&servers);
        session.announce(&servers);
        Ok(Value::Null)
    })
}

/// A Windows Hello key also goes from Windows Hello; if that fails, it still goes from here.
#[cfg(windows)]
fn forget_hello_credential(session: &Session, store: &Store, id: KeyId) {
    use russh::keys::ssh_key::private::KeypairData;
    use sshcraft_model::keytypes::KeyKind;
    let Ok(Some(record)) = store.key_record(id) else { return };
    if record.kind != KeyKind::Device {
        return;
    }
    let Ok(vault) = session.vault() else { return };
    let Ok(text) = vault.stored_openssh(store, id) else { return };
    let Ok(stored) = russh::keys::PrivateKey::from_openssh(text.as_str()) else { return };
    let handle = match stored.key_data() {
        KeypairData::SkEcdsaSha2NistP256(sk) => sk.key_handle().to_vec(),
        KeypairData::SkEd25519(sk) => sk.key_handle().to_vec(),
        _ => Vec::new(),
    };
    let _ = sshcraft_fido::webauthn::delete_platform_credential(&handle);
}

fn run_passcli_status(ctx: Ctx, _: Value) -> Out {
    blocking_run("key.passcli_status", move || {
        let status = ctx.session.pass_cli().map_or(PassCliStatus::NotInstalled, |cli| cli.status());
        to_value(&status)
    })
}

fn run_passcli_list(ctx: Ctx, _: Value) -> Out {
    blocking_run("key.passcli_list", move || {
        let cli = ctx.session.pass_cli().ok_or(PassCliError::NotInstalled)?;
        to_value(&cli.list_ssh_keys()?)
    })
}

#[derive(Deserialize)]
struct PassImportParams {
    share_id: String,
    item_id: String,
    name: String,
}

fn run_passcli_import(ctx: Ctx, params: Value) -> Out {
    blocking_run("key.passcli_import", move || {
        let PassImportParams { share_id, item_id, name } = parse("key.passcli_import", params)?;
        let vault = ctx.session.vault()?;
        let cli = ctx.session.pass_cli().ok_or(PassCliError::NotInstalled)?;
        let info = passcli::import_from_pass_cli(cli.as_ref(), &vault, &ctx.session.shared_store(), &share_id, &item_id, &name)?;
        ctx.session.announce(&[]);
        to_value(&info)
    })
}
