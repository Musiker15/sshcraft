# SSHCraft Teilplan 10: Schlüssel erweitert Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Schlüssel lassen sich als Ed25519, ECDSA und RSA erzeugen, mit oder ohne gespeicherte Passphrase ablegen, per Klick in `authorized_keys` eines Hosts eintragen, mit einem SSH-Zertifikat verbinden, und es gibt FIDO2-Hardwareschlüssel sowie gerätegebundene Schlüssel über Windows Hello (TPM).

**Architecture:** Der Tresor speichert jeden Schlüssel weiter als OpenSSH-Text, versiegelt mit der Vault-Chiffre. Neu ist, dass dieser Text selbst mit einer Passphrase verschlüsselt sein darf (`protected`). Dann fragt SSHCraft beim Verbinden danach, und Sync und Teilen tragen den verschlüsselten Text weiter. Ein Zertifikat ist öffentliche Information und hängt als OpenSSH-Zeile am Schlüssel. FIDO2 und Windows Hello sind beide `sk-`-Schlüssel nach OpenSSH. Die Schlüsseldatei enthält nur Key-Handle, Anwendung `ssh:` und Flags, signiert wird über die Windows-WebAuthn-API (`webauthn.dll`): einmal mit externem Token (cross-platform), einmal mit dem Plattform-Authenticator Windows Hello. Ein neues Crate `sshcraft-fido` hält das plattformneutrale Format und einen Software-Authenticator für Tests. Der SSH-Kern bekommt einen allgemeinen `KeySigner`, über den das Crate signiert. Gerätegebundene Schlüssel (`KeyKind::Device`) verlassen das Gerät nie.

**Tech Stack:** wie Teilplan 9; neu: `ciborium` (COSE-Schlüssel aus der Attestierung), `sha2` und `ed25519-dalek` (schon im Workspace), `p256` mit `ecdsa` (DER nach r/s, Software-Authenticator), `windows` mit `Win32_Networking_WindowsWebServices`. Versionen vor dem Eintragen mit `cargo info` prüfen und die neueste nehmen; `p256` in der Version, die `ssh-key` 0.7.0-rc.11 schon zieht (0.14.0, laut `Cargo.lock`), damit es nur eine gibt. RSA-Erzeugung kommt aus `ssh-key` (`RsaKeypair::random`), russh 0.65.1 zieht `ssh-key` mit `rsa`, `p256`, `p384`, `p521`, `encryption` und `ppk` (geprüft am 12.11.2026). `russh` kann eigene Signierer (`authenticate_publickey_with`, `authenticate_certificate_with`, Trait `russh::auth::Signer`) und Zertifikate (`authenticate_openssh_cert`); die Antwort eines Signierers ist `data || string(signature_blob)`, für `sk-` Signaturen mit Flags und Zähler (so in `keys/agent/client.rs`, `write_signature`).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`. Maßgeblich sind:
- Abschnitt 2 "Schlüssel": Erzeugen und Importieren von Ed25519, ECDSA und RSA, Passphrase wahlweise speichern; öffentlichen Schlüssel per Klick in `authorized_keys`; SSH-Zertifikate zusammen mit dem privaten Schlüssel; FIDO2 mit Berührung und PIN; gerätegebundene Schlüssel, die das Gerät nie verlassen und nicht synchronisiert werden (hier: Windows Hello mit TPM; Android und iOS kommen mit ihren Teilplänen).
- Abschnitt 3: Windows Hello vor gespeicherten Schlüsseln.
- Abschnitt 5: Schlüsselquellen.
- Abschnitt 8: Teilen mit oder ohne Schlüssel.
- Abschnitt 11: nichts Geheimes im Klartext auf der Platte oder in Logs.

## Global Constraints

- Alle Regeln der Teilpläne 1 bis 9 gelten weiter:
  - Arbeitsbaum unter `G:\Claude\worktrees`.
  - Code auf Englisch, Commits auf Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8.
  - Neueste Paketversionen, `cargo update` ohne `--workspace`, `npm outdated`.
  - Gegenproben für Regeln, Rechte und Datenwege.
  - Ohne Remote nach grünen Tests lokal in `main` mergen.
  - Oberflächentexte nur über die Übersetzung (en, de), Fehler aus Rust nur als `ErrorCode`.
- **Schlüsseltypen beim Erzeugen:** Ed25519 (Vorgabe), ECDSA mit `nistp256`, `nistp384`, `nistp521`, RSA mit 3072 (Vorgabe für RSA) oder 4096 Bit. Andere RSA-Größen lehnt der Tresor ab (`VaultError::UnsupportedKeyType`). Importiert wird weiter alles, was `parse_private_key` liest.
- **Passphrase:**
  - Sie steht nie in der Datenbank, in einem Log, in einem Fehler oder in einem Rückgabewert an die Oberfläche. Im Speicher als `Zeroizing<String>`.
  - "Nicht speichern" heißt: Der Tresor hält den mit der Passphrase verschlüsselten OpenSSH-Text (`PrivateKey::encrypt`), versiegelt wie jeden Schlüssel. Ein entschlüsselter Schlüssel wird dabei nie zusätzlich abgelegt.
  - Eine leere Passphrase gilt als keine.
  - Beim Verbinden fragt SSHCraft höchstens 3 Mal (`MAX_PASSPHRASE_TRIES`), danach `ErrorCode::WrongPassphrase`. Abbruch ergibt `AuthCancelled`.
- **Windows-Hello-Sperre (`KeyGate`)** gilt nur für Schlüssel, deren Geheimnis ohne weitere Prüfung im Tresor liegt. Für Schlüssel mit nicht gespeicherter Passphrase und für `sk-`Schlüssel (der Authenticator prüft selbst) entfällt sie.
- **Gerätegebundene Schlüssel (`KeyKind::Device`):**
  - Ihr Ort bleibt immer "nur auf diesem Gerät".
  - Sie gehen in keinen Sync-Datensatz, in keine Freigabe (auch nicht "mit Schlüssel") und in keinen Export.
  - Ein Host, der auf einem anderen Gerät auf sie zeigt, meldet `ErrorCode::KeyNotOnDevice`, wie bisher.
- **FIDO2 und Windows Hello:**
  - Anwendung (`application`, bei WebAuthn die RP-ID) ist immer `ssh:` (`SSH_APPLICATION`), wie bei OpenSSH.
  - Algorithmus `sk-ecdsa-sha2-nistp256@openssh.com` (Vorgabe) oder `sk-ssh-ed25519@openssh.com`. Windows Hello kann nur ECDSA P-256.
  - Flags: `0x01` (Berührung) immer, `0x04` (PIN oder Biometrie) wenn gewählt; Vorgabe ist mit PIN, für Windows Hello immer.
  - Eine Assertion mit Erweiterungen (Flag `0x80` in den Authenticator-Daten) wird abgelehnt.
  - Nur unter Windows. Auf anderen Systemen liefern die Befehle `ErrorCode::NotSupported`, bis die Plattform-Teilpläne kommen.
- **Zertifikate:**
  - Das Zertifikat muss zum öffentlichen Schlüssel passen (gleiche `KeyData`), sonst `VaultError::CertificateMismatch`.
  - Nur Benutzerzertifikate (`CertType::User`).
  - Beim Verbinden geht ein Zertifikat nur hinaus, wenn es gerade gültig ist (`valid_after <= jetzt < valid_before`). Sonst und nach Ablehnung durch den Server folgt der Schlüssel allein, und die Statuszeile zeigt `ErrorCode::CertificateExpired` als Hinweis, wenn es abgelaufen war.
- **`authorized_keys`:**
  - Eingetragen wird nur der öffentliche Schlüssel aus dem Tresor oder vom Agent, nie etwas aus der Oberfläche.
  - Der Kommentar wird auf `[A-Za-z0-9@._-]` gekürzt (höchstens 64 Zeichen).
  - Ist der Schlüssel schon eingetragen (gleicher Typ und gleiche Base64-Daten, egal mit welchen Optionen davor), kommt keine zweite Zeile dazu.

## Review Focus

1. **Ein gerätegebundener Schlüssel verlässt das Gerät:** über den Sync, über eine Freigabe "mit Schlüssel", über das Verschieben des Schlüssels oder eines Hosts an einen Server oder über den Export. Erwartet wird, dass er nirgends ankommt und nur das Verschieben des Schlüssels selbst mit `StoreError::DeviceBound` scheitert. Tests in Task 2.
2. **Passphrase-Abfrage geht schief:** falsche Passphrase, Abbruch, Schlüssel auf einem Sprung-Host statt am Ziel. Erwartet werden höchstens 3 Fragen mit dem Namen des Schlüssels, danach `WrongPassphrase`, und kein entschlüsselter Schlüssel in der Datenbank. Test in Task 8.
3. **Kaputte oder unpassende Zertifikate:** fremder Schlüssel, Host- statt Benutzerzertifikat, abgelaufen, Text ohne Zertifikat. Erwartet wird, dass das Anhängen mit klarem Fehler scheitert und ein abgelaufenes Zertifikat beim Verbinden übersprungen wird. Tests in Task 3 und 4.
4. **`authorized_keys` auf echten Servern:** Datei ohne Zeilenumbruch am Ende, Schlüssel schon mit Optionen eingetragen, Kommentar mit Sonderzeichen, Server ohne `sh` (Windows-OpenSSH). Erwartet werden keine kaputte Zeile, kein Duplikat, keine Shell-Ausführung aus dem Kommentar und `ErrorCode::InstallKeyUnsupported` statt eines stillen Erfolgs. Tests in Task 5, der Lauf mit echtem `sh`.
5. **FIDO2-Schlüssel ohne Token oder abgebrochene Berührung:** Erwartet wird `ErrorCode::SecurityKeyCancelled` beziehungsweise `SecurityKeyFailed` und kein Hängen. Ein synchronisierter FIDO2-Schlüssel ist auf dem zweiten Gerät nutzbar, sobald der Token steckt. Tests in Task 6 und 8.

---

## Dateistruktur

```
crates/sshcraft-core/src/store.rs            Schema 7: keys.kind, keys.protected, keys.certificate; DeviceBound
crates/sshcraft-core/src/vault.rs            KeySpec, generate, Passphrase, Zertifikate, Device-Schlüssel
crates/sshcraft-core/src/keytypes.rs         KeySpec, KeyKind, CertificateInfo (neu)
crates/sshcraft-core/src/authorized_keys.rs  Befehl und Auswertung für authorized_keys (neu)
crates/sshcraft-core/src/import/keyfiles.rs  "<datei>-cert.pub" neben einem Schlüssel
crates/sshcraft-core/src/sshconfig.rs        CertificateFile
crates/sshcraft-sync/src/items.rs            Schlüssel als gespeicherter Text plus Zertifikat; Device-Schlüssel nie
crates/sshcraft-sync/src/shares.rs           Device-Schlüssel gehen in keine Freigabe
crates/sshcraft-ssh/src/lib.rs               Credential::Signer, KeySigner, Session::exec mit Status
crates/sshcraft-ssh/src/auth.rs              Zertifikat zuerst, dann Schlüssel; Signierer
crates/sshcraft-ssh/src/testing.rs           Testserver: CA, exec-Antwort, empfangene Befehle
crates/sshcraft-fido/                        neu: Format, Authenticator-Trait, SkSigner, Software-Authenticator, WebAuthn (Windows)
app/src-tauri/src/sessions.rs                Schlüssel auflösen (Passphrase, sk, Zertifikat), Schlüssel eintragen
app/src-tauri/src/commands/keys.rs           neue Befehle
app/src-tauri/src/errors.rs                  neue ErrorCodes
app/src/pages/KeysPage.tsx                   Erzeugen, Sicherheitsschlüssel, Details, Eintragen
app/src/components/{GenerateKeyDialog,SecurityKeyDialog,KeyDetails,InstallKeyDialog}.tsx (neu)
app/src/components/AuthPromptDialog.tsx      Passphrase-Frage
app/src/i18n/{en,de}.json
docs/keys.md (neu), docs/security.md, docs/sync.md, README.md, Fahrplan
```

---

### Task 1: Tresor: Schlüsseltypen, Passphrase, Schema 7

**Files:**
- Create: `crates/sshcraft-core/src/keytypes.rs`
- Modify: `crates/sshcraft-core/src/store.rs` (Schema 7, `KeyRecord`), `crates/sshcraft-core/src/vault.rs`, `crates/sshcraft-core/src/lib.rs`
- Test: `crates/sshcraft-core/src/vault_tests.rs`, `crates/sshcraft-core/src/store_tests.rs`

**Interfaces:**
- Produces:
  - `keytypes::KeySpec` (serde, `tag = "type"`, snake_case): `Ed25519`, `Ecdsa { curve: EcdsaCurveName }`, `Rsa { bits: u32 }`; `EcdsaCurveName { Nistp256, Nistp384, Nistp521 }`.
  - `keytypes::KeyKind { Vault, Device }` (serde snake_case, Speicherung als `"vault"`/`"device"`).
  - `KeyRecord` neu mit `kind: KeyKind`, `protected: bool`, `certificate: Option<String>`.
  - `KeyInfo` neu mit `kind`, `protected`, `certificate: Option<CertificateInfo>` (Task 3 füllt es, hier immer `None`).
  - `Vault::generate(&self, store, name: &str, spec: &KeySpec, passphrase: Option<&str>) -> Result<KeyInfo, VaultError>` ersetzt `generate_ed25519`.
  - `Vault::import_key(&self, store, name, text, passphrase: Option<&str>, save_passphrase: bool) -> Result<KeyInfo, VaultError>`.
  - `Vault::store_protected(&self, store, name, key: &PrivateKey, passphrase: &str) -> Result<KeyInfo, VaultError>`.
  - `Vault::private_key` liefert `Err(VaultError::PassphraseRequired)` für `protected`.
  - `Vault::unlock_key(&self, store, id, passphrase: &str) -> Result<PrivateKey, VaultError>`.
  - `Vault::remember_passphrase(&self, store, id, passphrase: &str) -> Result<KeyInfo, VaultError>` (danach nicht mehr `protected`).
  - `Vault::protect_key(&self, store, id, passphrase: &str) -> Result<KeyInfo, VaultError>`.
  - `Vault::stored_openssh(&self, store, id) -> Result<Zeroizing<String>, VaultError>` (der gespeicherte Text, verschlüsselt oder nicht).
  - `Vault::store_openssh_with_id(&self, store, id, name, openssh: &str, certificate: Option<&str>, location: Option<ServerId>) -> Result<KeyInfo, VaultError>` (für den Sync; `protected` ergibt sich aus `is_encrypted()`).
  - `Vault::store_device_key(&self, store, name, key: &PrivateKey) -> Result<KeyInfo, VaultError>` (`kind = Device`, Ort `None`).
  - `VaultError::{PassphraseRequired, UnsupportedKeyType, CertificateMismatch, InvalidCertificate}`.

- [ ] **Step 1: Write the failing tests** in `vault_tests.rs`:

```rust
#[test]
fn generates_every_supported_type() {
    // Ed25519 -> "ssh-ed25519"; Ecdsa Nistp256/384/521 -> "ecdsa-sha2-nistp256"/"...384"/"...521";
    // Rsa 3072 and 4096 -> "ssh-rsa" with that modulus size (check via key_data().rsa().n bits).
}
#[test]
fn rejects_rsa_sizes_outside_3072_and_4096() { /* 2048, 1024, 8192 -> UnsupportedKeyType */ }
#[test]
fn key_without_saved_passphrase_needs_it() {
    // import an encrypted OpenSSH key with save_passphrase = false
    // info.protected == true; private_key() -> PassphraseRequired
    // unlock_key(wrong) -> WrongPassphrase; unlock_key(right) -> same fingerprint
}
#[test]
fn protected_key_never_lands_decrypted_on_disk() {
    // open the SQLite file bytes after import with save_passphrase = false:
    // neither the unencrypted OpenSSH body nor the passphrase appear; stored_openssh() parses as is_encrypted()
}
#[test]
fn saved_passphrase_stores_plain_key() { /* save_passphrase = true -> protected == false, private_key works */ }
#[test]
fn remember_and_protect_switch_protection_and_keep_id_location_kind() {}
#[test]
fn generate_with_passphrase_is_protected() {}
#[test]
fn empty_passphrase_counts_as_none() { /* generate(.., Some("")) -> protected == false */ }
```

In `store_tests.rs`: `schema_7_adds_key_columns_with_defaults` (eine Datenbank in Version 6 mit einem Schlüssel öffnen: `kind == Vault`, `protected == false`, `certificate == None`).

- [ ] **Step 2: Run tests, expect FAIL** (`cargo test -p sshcraft-core vault store`): `generate` und die neuen Felder fehlen.

- [ ] **Step 3: Implement**
  - Schema 7: `ALTER TABLE keys ADD COLUMN kind TEXT NOT NULL DEFAULT 'vault'; ALTER TABLE keys ADD COLUMN protected INTEGER NOT NULL DEFAULT 0; ALTER TABLE keys ADD COLUMN certificate TEXT;` `SCHEMA_VERSION = 7`.
  - RSA über `ssh_key::private::RsaKeypair::random(rng, bits)` und `PrivateKey::new(KeypairData::from(..), comment)`, ECDSA über `PrivateKey::random(.., Algorithm::Ecdsa { curve })`. Kommentar ist der Name.
  - Geschützt speichern: `key.encrypt(&mut rand::rng(), passphrase)`, dann `to_openssh` und wie bisher versiegeln.
  - `Cargo.toml` (Workspace): `[profile.dev.package.num-bigint-dig]` und `[profile.dev.package.rsa]` mit `opt-level = 3`, damit RSA-4096 in Debug-Tests nicht Minuten dauert. Paketnamen vorher in `Cargo.lock` prüfen.
  - Alle Aufrufer von `generate_ed25519` und `import_key` anpassen (App, Tests).

- [ ] **Step 4: Run tests, expect PASS**; Gegenprobe: in `import_key` mit `save_passphrase = false` testweise den entschlüsselten Schlüssel speichern, `protected_key_never_lands_decrypted_on_disk` muss rot werden.

- [ ] **Step 5: Commit** `Tresor: ECDSA und RSA erzeugen, Passphrase wahlweise speichern`

---

### Task 2: Sync und Teilen: gespeicherter Text, gerätegebundene Schlüssel bleiben

**Files:**
- Modify: `crates/sshcraft-core/src/store.rs` (`set_location`, Ausgang), `crates/sshcraft-sync/src/items.rs`, `crates/sshcraft-sync/src/shares.rs`, `crates/sshcraft-sync/src/engine.rs` (wo Schlüssel eingereiht werden)
- Test: `crates/sshcraft-sync/tests/credentials.rs`, `crates/sshcraft-sync/tests/share_credentials.rs`, `crates/sshcraft-core/src/store_tests.rs`

**Interfaces:**
- Consumes: `Vault::stored_openssh`, `Vault::store_openssh_with_id`, `KeyKind` (Task 1).
- Produces:
  - `ItemBody::Key { id, name, openssh, #[serde(default)] certificate: Option<String> }`: `openssh` ist der gespeicherte Text, auch verschlüsselt.
  - `StoreError::DeviceBound`: `Store::set_location(ItemKind::Key, id, Some(_))` für einen Device-Schlüssel.

- [ ] **Step 1: Write the failing tests**
  - `credentials.rs`:
    - `protected_key_syncs_encrypted_and_stays_protected`: Gerät A importiert ohne gespeicherte Passphrase, Gerät B bekommt `protected == true` und dieselbe Fingerprint; der Sync-Klartext vor der Ende-zu-Ende-Verschlüsselung enthält den unverschlüsselten Körper nicht.
    - `certificate_travels_with_key`.
    - `device_key_never_syncs`: Host am Server mit `KeyRef::Stored` auf einen Device-Schlüssel. Gerät B bekommt den Host, aber keinen Schlüssel, und im Ausgang steht kein `Key`-Eintrag.
  - `share_credentials.rs`: `device_key_never_shared_even_with_keys` (Freigabe "mit Schlüssel"; der Empfänger hat den Host, aber keinen Schlüssel).
  - `store_tests.rs`: `device_key_cannot_move_to_server` erwartet `StoreError::DeviceBound`, der Ort bleibt `None`.

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement**
  - `items.rs` liest den gespeicherten Text statt `private_key` und schreibt eingehende Schlüssel mit `store_openssh_with_id`.
  - Jede Stelle, die einen Schlüssel für einen Server oder eine Freigabe einreiht oder als Kopie schreibt, überspringt `KeyKind::Device`. Die Stellen findet `grep -n "ItemKind::Key" crates/sshcraft-sync/src crates/sshcraft-core/src`.
  - Ein Host, der mit einem Device-Schlüssel an einen Server wandert, nimmt den Schlüssel nicht mit, scheitert aber selbst nicht.

- [ ] **Step 4: Run, expect PASS**; Gegenprobe: das Überspringen in der Freigabe entfernen, `device_key_never_shared_even_with_keys` wird rot.

- [ ] **Step 5: Commit** `Sync: Schlüssel mit Passphrase und Zertifikat, gerätegebundene Schlüssel bleiben auf dem Gerät`

---

### Task 3: Zertifikate im Tresor und beim Import

**Files:**
- Modify: `crates/sshcraft-core/src/keytypes.rs`, `crates/sshcraft-core/src/vault.rs`, `crates/sshcraft-core/src/import/keyfiles.rs`, `crates/sshcraft-core/src/sshconfig.rs`
- Test: `crates/sshcraft-core/src/vault_tests.rs`, `crates/sshcraft-core/src/import/tests.rs`, `crates/sshcraft-core/src/sshconfig_tests.rs`

**Interfaces:**
- Produces:
  - `keytypes::CertificateInfo { key_id: String, principals: Vec<String>, valid_after: u64, valid_before: u64, ca_fingerprint: String }` (serde camelCase wie die übrigen `KeyInfo`-Felder, `valid_before == u64::MAX` heißt "unbegrenzt").
  - `Vault::attach_certificate(&self, store, id, text: &str) -> Result<KeyInfo, VaultError>`, `Vault::detach_certificate(&self, store, id) -> Result<KeyInfo, VaultError>`.
  - `vault::certificate(store, id) -> Result<Option<russh::keys::Certificate>, VaultError>`.
  - Keyfile-Import: liegt neben `<pfad>` eine `<pfad>-cert.pub`, wird sie angehängt, wenn sie passt; passt sie nicht, wird der Schlüssel trotzdem importiert und der Befund in der Import-Zusammenfassung gemeldet. ssh-config-Import: `CertificateFile` (mit `~` und `%d`, `%u`, `%h` wie bei `IdentityFile`) gilt für das `IdentityFile` desselben Blocks.

- [ ] **Step 1: Write the failing tests**
  - `attaches_matching_user_certificate`: Testzertifikat mit `ssh_key::certificate::Builder`, CA ist ein eigener Ed25519-Schlüssel. `KeyInfo.certificate` hat Principals, Gültigkeit und den CA-Fingerprint.
  - `rejects_certificate_for_other_key`: `CertificateMismatch`.
  - `rejects_host_certificate`: `InvalidCertificate`.
  - `rejects_garbage_certificate`: `InvalidCertificate`.
  - `detach_removes_certificate`.
  - Import: `keyfile_import_picks_up_cert_sibling`, `mismatching_cert_sibling_is_reported_not_fatal`, `sshconfig_certificatefile_attaches`.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** mit `Certificate::from_openssh`, `cert.public_key() == key.public_key().key_data()` und `cert.cert_type() == CertType::User`. Gespeichert wird die Zeile, wie sie kam (getrimmt).
- [ ] **Step 4: Run, expect PASS**; Gegenprobe: den Vergleich der `KeyData` entfernen, `rejects_certificate_for_other_key` wird rot.
- [ ] **Step 5: Commit** `Tresor: SSH-Zertifikate am Schlüssel, Import neben der Schlüsseldatei und aus ssh config`

---

### Task 4: SSH-Kern: eigene Signierer und Anmeldung mit Zertifikat

**Files:**
- Modify: `crates/sshcraft-ssh/src/lib.rs`, `crates/sshcraft-ssh/src/auth.rs`, `crates/sshcraft-ssh/src/testing.rs`
- Test: `crates/sshcraft-ssh/tests/auth.rs`

**Interfaces:**
- Produces:
  - `#[async_trait] pub trait KeySigner: Send { async fn sign(&mut self, data: &[u8]) -> Result<Vec<u8>, SshError>; }`: liefert den SSH-Signatur-Blob (`string algorithm, string signature` und bei `sk-` zusätzlich `byte flags, uint32 counter`), nicht die Länge davor.
  - `Credential::Signer { public_key: russh::keys::PublicKey, signer: Box<dyn KeySigner> }`.
  - `Auth { key, password, certificate: Option<russh::keys::Certificate> }`, `#[derive(Default)]`.
  - `SshError::{SignerCancelled, SignerFailed(String)}`.
  - Testserver: `TestAuth.ca: Option<PublicKey>` (nimmt Benutzerzertifikate dieser CA an, Principal muss den Benutzer enthalten), `TestServer::received_certificates() -> usize`.

- [ ] **Step 1: Write the failing tests** in `tests/auth.rs`:
  - `signer_signs_in`: ein `KeySigner`, der mit einem normalen Ed25519-Schlüssel signiert (`ssh_key::PrivateKey::try_sign`, Blob über `ssh_encoding`), meldet sich an.
  - `signer_cancel_is_reported`: liefert `SignerCancelled`, `connect` endet mit `SshError::SignerCancelled`.
  - `certificate_signs_in_where_key_alone_is_not_accepted`: `TestAuth { key: None, ca: Some(ca) }`.
  - `rejected_certificate_falls_back_to_key`: CA unbekannt, Schlüssel bekannt. Anmeldung gelingt, `received_certificates() == 1`.
  - `expired_certificate_is_not_sent`: `received_certificates() == 0`, Anmeldung über den Schlüssel.
  - `certificate_with_signer_signs_in`.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement**
  - Ein Adapter `struct SignerAdapter<'a>(&'a mut dyn KeySigner)` implementiert `russh::auth::Signer`: `data || string(blob)`, mit Fehlertyp `SignerAuthError { Send, Signer(SshError) }`.
  - `publickey()`: Ist ein Zertifikat da und gerade gültig, kommt erst das Zertifikat (`authenticate_openssh_cert` beziehungsweise `authenticate_certificate_with`). Bei `Failure`, solange `publickey` angeboten wird, folgt der Schlüssel allein.
  - Zeitlimit für Signierer wie beim Agent (`AGENT_SIGN_TIMEOUT`).
  - Ob ein Zertifikat abgelaufen war, gibt `connect_route` an die App zurück, damit sie den Hinweis zeigt. Ein Feld `certificate_expired: bool` in `Session` reicht, gesetzt für das Ziel.
- [ ] **Step 4: Run, expect PASS**; Gegenprobe: die Gültigkeitsprüfung entfernen, `expired_certificate_is_not_sent` wird rot.
- [ ] **Step 5: Commit** `SSH: eigene Signierer und Anmeldung mit Zertifikat`

---

### Task 5: Öffentlichen Schlüssel in `authorized_keys` eintragen

**Files:**
- Create: `crates/sshcraft-core/src/authorized_keys.rs`
- Modify: `crates/sshcraft-ssh/src/testing.rs` (exec-Antwort einstellbar, Befehle aufzeichnen)
- Test: `crates/sshcraft-core/src/authorized_keys.rs` (Modul `tests`), `crates/sshcraft-ssh/tests/route.rs` oder neue `tests/exec.rs`

**Interfaces:**
- Produces:
  - `pub fn install_command(public_key: &russh::keys::PublicKey, verify_required: bool) -> String`. `verify_required` setzt `verify-required ` vor die Zeile; die App gibt es für `sk-`Schlüssel mit Flag `0x04` an.
  - `pub enum InstallOutcome { Added, AlreadyPresent }` (serde snake_case).
  - `pub fn parse_outcome(output: &[u8]) -> Result<InstallOutcome, AuthorizedKeysError>`; `AuthorizedKeysError::Unsupported` wenn keine Marke kommt.
  - Testserver: `TestServer::set_exec_output(&self, output: &str)`, `TestServer::received_commands() -> Vec<String>`.

Der Befehl läuft unter `sh -c '…'`. Innen stehen nur doppelte Anführungszeichen um Werte, die ausschließlich aus `[A-Za-z0-9+/=@._ -]` bestehen. Ablauf im Skript:

```sh
umask 077; d="$HOME/.ssh"; f="$d/authorized_keys"
mkdir -p "$d" && touch "$f" || exit 1
if grep -qF "<type> <base64>" "$f"; then echo SSHCRAFT_PRESENT; exit 0; fi
if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi
printf "%s\n" "<line>" >> "$f" && echo SSHCRAFT_ADDED
```

- [ ] **Step 1: Write the failing tests**
  - `comment_is_sanitized` (Kommentar `a'b;$(rm)` ergibt `abrm`).
  - `parse_outcome_reads_markers_and_rejects_others`.
  - `script_runs_in_sh`: läuft nur, wenn `sh` im `PATH` ist, sonst `eprintln!` und Ende. `HOME` zeigt auf ein temporäres Verzeichnis. Geprüft wird:
    - der erste Lauf ergibt `Added`, der zweite `AlreadyPresent`, die Datei hat genau eine Zeile;
    - eine Datei ohne Zeilenumbruch am Ende bekommt eine eigene Zeile;
    - eine vorhandene Zeile `no-pty ssh-ed25519 AAAA… alt` zählt als vorhanden;
    - mit `verify_required` beginnt die Zeile mit `verify-required sk-`.
  - SSH: `install_runs_one_command_over_exec` gegen den Testserver mit `set_exec_output("SSHCRAFT_ADDED\n")`: genau ein empfangener Befehl, der mit `sh -c '` beginnt.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** Die Zeile wird aus `public_key.algorithm()` und den Base64-Daten (`to_openssh` ohne Kommentar) neu gebaut, nie aus einem übergebenen Text.
- [ ] **Step 4: Run, expect PASS** (Git-Bash-`sh` liegt auf diesem Rechner im `PATH`; prüfen, dass `script_runs_in_sh` wirklich läuft und nicht übersprungen wird). Gegenprobe: das Bereinigen des Kommentars entfernen, `comment_is_sanitized` wird rot; die Prüfung auf den fehlenden Zeilenumbruch entfernen, der Testfall dazu wird rot.
- [ ] **Step 5: Commit** `Kern: öffentlichen Schlüssel in authorized_keys eintragen`

---

### Task 6: Crate `sshcraft-fido`: Format, Signierer, Software-Authenticator

**Files:**
- Create: `crates/sshcraft-fido/{Cargo.toml, src/lib.rs, src/cose.rs, src/signature.rs, src/soft.rs}`; in den Workspace aufnehmen
- Test: `crates/sshcraft-fido/src/*` (Module `tests`), `crates/sshcraft-fido/tests/signin.rs`

**Interfaces:**
- Consumes: `sshcraft_ssh::{KeySigner, Credential, SshError}` (Task 4).
- Produces:
  - `pub const SSH_APPLICATION: &str = "ssh:"`; `pub const FLAG_USER_PRESENCE: u8 = 0x01`; `pub const FLAG_USER_VERIFICATION: u8 = 0x04`.
  - `pub enum SkAlgorithm { EcdsaP256, Ed25519 }` (serde snake_case), `pub enum Attachment { CrossPlatform, Platform }`.
  - `pub struct CreateOptions { pub algorithm: SkAlgorithm, pub attachment: Attachment, pub verify_user: bool, pub user_name: String }`.
  - `pub struct Attestation { pub authenticator_data: Vec<u8>, pub credential_id: Vec<u8> }`, `pub struct Assertion { pub authenticator_data: Vec<u8>, pub signature: Vec<u8> }`.
  - `pub enum FidoError { NotSupported, Cancelled, Failed(String), BadResponse(String) }`.
  - `#[async_trait] pub trait Authenticator: Send + Sync`:
    - `async fn make_credential(&self, application: &str, options: &CreateOptions, client_data: &[u8]) -> Result<Attestation, FidoError>`
    - `async fn get_assertion(&self, application: &str, credential_id: &[u8], client_data: &[u8], verify_user: bool, attachment: Attachment) -> Result<Assertion, FidoError>`
  - `pub async fn create_key(auth: &dyn Authenticator, options: &CreateOptions, comment: &str) -> Result<PrivateKey, FidoError>`: `sk-`Schlüssel mit Anwendung `ssh:`, Flags aus `verify_user`, Key-Handle = `credential_id`; der öffentliche Schlüssel kommt aus dem COSE-Schlüssel in `authenticator_data`.
  - `pub fn ssh_signature(algorithm: SkAlgorithm, assertion: &Assertion) -> Result<Vec<u8>, FidoError>`.
  - `pub fn is_security_key(key: &PrivateKey) -> bool`.
  - `pub struct SkSigner { .. }` mit `SkSigner::new(auth: Arc<dyn Authenticator>, key: &PrivateKey, attachment: Attachment) -> Result<Self, FidoError>`, implementiert `KeySigner`. `Cancelled` wird zu `SshError::SignerCancelled`, alles andere zu `SignerFailed`.
  - Feature `testing`: `SoftAuthenticator::new(attachment)` erzeugt echte P-256- beziehungsweise Ed25519-Schlüssel und signiert `authData || SHA-256(client_data)` wie ein Token. Dazu `assertions() -> usize` und `set_cancel(bool)`.

Authenticator-Daten: `rpIdHash(32) | flags(1) | counter(4, BE)`, bei der Attestierung dazu `aaguid(16) | credIdLen(2, BE) | credId | COSE_Key (CBOR)`. COSE: ES256 `{1: 2, 3: -7, -1: 1, -2: x(32), -3: y(32)}`, EdDSA `{1: 1, 3: -8, -1: 6, -2: x(32)}`. Der SSH-Blob für ECDSA: `string "sk-ecdsa-sha2-nistp256@openssh.com", string(mpint r, mpint s), byte flags, uint32 counter`; die DER-Signatur wird über `p256::ecdsa::Signature::from_der` zerlegt. Für Ed25519 ist die Signatur die rohen 64 Bytes.

- [ ] **Step 1: Write the failing tests**
  - `cose_parses_es256_and_eddsa`, `cose_rejects_wrong_curve_and_short_coordinates`.
  - `assertion_with_extensions_is_rejected` (Flag `0x80`), `assertion_with_wrong_rp_hash_is_rejected` (`rpIdHash != SHA-256("ssh:")`).
  - `created_key_carries_application_flags_and_handle`.
  - `tests/signin.rs`: `sk_ecdsa_signs_in` und `sk_ed25519_signs_in` gegen den `TestServer` aus `sshcraft-ssh`, der den öffentlichen `sk-`Schlüssel kennt und über `ssh-key` prüft. Dazu `cancel_ends_with_signer_cancelled` und `signature_from_other_key_is_rejected`.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** in den genannten Dateien. Die neueste `ciborium` mit `cargo info` prüfen.
- [ ] **Step 4: Run, expect PASS**; Gegenprobe: die Prüfung des `rpIdHash` entfernen, `assertion_with_wrong_rp_hash_is_rejected` wird rot.
- [ ] **Step 5: Commit** `FIDO2: sk-Schlüssel nach OpenSSH, Signierer und Software-Authenticator`

---

### Task 7: WebAuthn unter Windows (Token und Windows Hello)

**Files:**
- Create: `crates/sshcraft-fido/src/webauthn.rs` (`#[cfg(windows)]`)
- Modify: `crates/sshcraft-fido/Cargo.toml` (`[target.'cfg(windows)'.dependencies] windows` mit `Win32_Networking_WindowsWebServices`, `Win32_Foundation`)
- Test: Modul `tests` in `webauthn.rs`

**Interfaces:**
- Produces:
  - `pub struct WebAuthn { pub hwnd: isize }`, implementiert `Authenticator`.
  - `pub fn webauthn_available() -> bool` (API-Version ab 1).
  - `pub fn platform_available() -> bool` (`WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable`).
  - `pub fn delete_platform_credential(credential_id: &[u8]) -> Result<(), FidoError>`: API-Version ab 4, sonst `NotSupported`.

Werte für die Aufrufe:
- RP `{ id: "ssh:", name: "SSHCraft" }`, Benutzer mit 32 zufälligen Bytes als ID und `user_name` als Name und Anzeigename.
- `pwszHashAlgId = "SHA-256"`, `client_data` als `pbClientDataJSON` (Windows hasht die Bytes, so wird der Client-Data-Hash `SHA-256(nachricht)` wie bei OpenSSH).
- Algorithmus `-7` oder `-8`, Anhang `CROSS_PLATFORM` oder `PLATFORM`.
- Benutzerprüfung `REQUIRED` oder `DISCOURAGED`, `bRequireResidentKey = false` für Token.
- Zeitlimit 120 s.
- Ergebnisse mit `WebAuthNFreeCredentialAttestation` beziehungsweise `WebAuthNFreeAssertion` freigeben, auch im Fehlerfall.
- Ein abgebrochener Dialog (`NTE_USER_CANCELLED` 0x80090036, `HRESULT_FROM_WIN32(ERROR_CANCELLED)` 0x800704C7, `NotAllowedError` aus `WebAuthNGetErrorName`) wird zu `FidoError::Cancelled`.
- Der Aufruf blockiert und läuft deshalb in `tokio::task::spawn_blocking`.

- [ ] **Step 1: Write the failing tests:** `cancel_codes_map_to_cancelled` (reine Abbildung von HRESULT und Fehlername), `api_version_is_reported` (ruft `webauthn_available()` auf; auf Windows 10 ab 1903 wahr).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** Die Strukturen leben im Speicher, bis der Aufruf zurück ist (Zeiger in `Vec`/`HSTRING`, die bis dahin gehalten werden).
- [ ] **Step 4: Run, expect PASS.** Echte Tokens prüft Moritz von Hand (Task 9, Schritt 4), weil der Test keinen Token drücken kann.
- [ ] **Step 5: Commit** `FIDO2: WebAuthn unter Windows für Token und Windows Hello`

---

### Task 8: App: Schlüssel auflösen, Passphrase fragen, Befehle

**Files:**
- Modify: `app/src-tauri/src/sessions.rs`, `app/src-tauri/src/state.rs`, `app/src-tauri/src/lib.rs`, `app/src-tauri/src/errors.rs`, `app/src-tauri/src/commands/keys.rs`, `app/src-tauri/src/commands/sessions.rs`, `app/src-tauri/Cargo.toml`, `crates/sshcraft-ssh/src/auth.rs` (`AuthRequest`)
- Test: `app/src-tauri/src/sessions_tests.rs`, `app/src-tauri/src/commands/keys_tests.rs` (neu, falls die Befehle Logik tragen)

**Interfaces:**
- Consumes: alles aus Task 1 bis 7.
- Produces:
  - `AuthRequest.passphrase_for: Option<String>` (Name des Schlüssels, serde `skip_serializing_if = "Option::is_none"`).
  - `SessionManager::install_key(&self, host_id: HostId, key: KeyRef) -> Result<InstallOutcome, (ErrorCode, Option<String>)>`: verbindet über die normale Route mit allen Abfragen, öffnet keine Shell und führt `install_command` aus.
  - Tauri-Befehle:
    - `generate_key(name, spec: KeySpec, passphrase: Option<String>)`
    - `import_key(.., save_passphrase: bool)`
    - `security_key_support() -> SecuritySupport { fido2: bool, hello: bool }`
    - `create_security_key(name, kind: SecurityKeyKind /* fido2 | hello */, algorithm: SkAlgorithm, verify_user: bool) -> KeyInfo`
    - `attach_certificate(id, text)`, `detach_certificate(id)`
    - `remember_passphrase(id, passphrase)`, `protect_key(id, passphrase)`
    - `install_key(host_id, key: KeyRef) -> InstallOutcome`
  - `ErrorCode::{WrongPassphrase, CertificateMismatch, InvalidCertificate, CertificateExpired, UnsupportedKeyType, SecurityKeyCancelled, SecurityKeyFailed, InstallKeyUnsupported, InstallKeyFailed, DeviceBound}`; `NotSupported` und `KeyNotOnDevice` bleiben.
  - `Context.security: Arc<dyn sshcraft_fido::Authenticator>` (Windows: `WebAuthn { hwnd }`, Tests: `SoftAuthenticator`).

Auflösen eines gespeicherten Schlüssels in `hop()`:
1. Kein Datensatz: `KeyNotOnDevice`.
2. `sk-`Schlüssel: Tresor öffnen ohne `KeyGate`, dann `Credential::Signer` mit `SkSigner`. `Device` nutzt `Attachment::Platform`, sonst `CrossPlatform`.
3. `protected`: Passphrase über `ctx.auth` fragen (`passphrase_for = Some(name)`, eine verdeckte Zeile), höchstens 3 Mal, dann `WrongPassphrase`. Ohne `KeyGate`.
4. Sonst wie bisher mit `KeyGate`.

Das Zertifikat kommt in `Auth.certificate`. `Session.certificate_expired` setzt in der Statuszeile den Hinweis `CertificateExpired`, auf demselben Weg wie `AgentForwardingUnavailable`. Beim Löschen eines `Device`-Schlüssels ruft die App unter Windows `delete_platform_credential` auf; ein Fehler dabei hält das Löschen nicht auf.

- [ ] **Step 1: Write the failing tests** in `sessions_tests.rs` (Testserver, Test-Prompter):
  - `protected_key_asks_for_passphrase_once_and_connects`: ein Prompt mit `passphrase_for == Some("deploy")`, `KeyGate` nicht gefragt.
  - `wrong_passphrase_three_times_ends_with_wrong_passphrase`: genau 3 Prompts.
  - `cancelled_passphrase_ends_with_auth_cancelled`.
  - `passphrase_prompt_names_the_jump_host_key` (Schlüssel am Sprung-Host).
  - `security_key_signs_without_key_gate` (Software-Authenticator, `KeyGate` zählt 0 Abfragen).
  - `expired_certificate_notice_reaches_status`.
  - `install_key_adds_line_via_exec` (Testserver mit `set_exec_output`) und `install_key_reports_unsupported_on_unknown_output`.
  - `device_key_on_other_device_reports_key_not_on_device`.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
  - Den Verbindungsteil von `connect_once` als `connect_session(ctx, host_id) -> Result<(Session, Option<ErrorCode>), Failure>` herausziehen. `connect_once` und `install_key` nutzen ihn beide.
  - `errors.rs` bildet die neuen `VaultError`, `FidoError` und `StoreError::DeviceBound` ab.
- [ ] **Step 4: Run, expect PASS**; Gegenprobe: die Grenze von 3 Versuchen auf 4 setzen, `wrong_passphrase_three_times_ends_with_wrong_passphrase` wird rot; `KeyGate` für `protected` wieder einschalten, `protected_key_asks_for_passphrase_once_and_connects` wird rot.
- [ ] **Step 5: Commit** `App: Passphrase beim Verbinden, Sicherheitsschlüssel, Zertifikate, Schlüssel eintragen`

---

### Task 9: Oberfläche

**Files:**
- Create: `app/src/components/{GenerateKeyDialog,SecurityKeyDialog,KeyDetails,InstallKeyDialog}.tsx`
- Modify: `app/src/pages/KeysPage.tsx`, `app/src/components/AuthPromptDialog.tsx`, `app/src/api.ts`, `app/src/i18n/{en,de}.json`, `app/src/shell/ContextMenu.tsx` (Eintrag am Host), `app/src/components/SessionStatusLine.tsx`
- Test: `app/src/**/*.test.ts(x)` nach dem Muster der vorhandenen vitest-Tests

**Inhalt:**
- **Erzeugen:** Typ (Ed25519, ECDSA mit Kurve, RSA mit 3072 oder 4096), Name, Passphrase optional, darunter "Passphrase speichern" (Vorgabe an).
- **Import:** bei verschlüsselten Schlüsseln derselbe Schalter.
- **Sicherheitsschlüssel:** "FIDO2-Schlüssel" und "Windows Hello", nur sichtbar, wenn `security_key_support` es meldet. Algorithmus nur bei FIDO2, "PIN bei jeder Anmeldung" bei FIDO2 (Vorgabe an). Während des Wartens steht ein Hinweis, den Token zu berühren.
- **Details eines Schlüssels:**
  - Art: Tresor, gerätegebunden oder Sicherheitsschlüssel, mit Marke "bleibt auf diesem Gerät".
  - Passphrase: "wird abgefragt" mit den Aktionen "Passphrase speichern" und "Mit Passphrase schützen".
  - Zertifikat: Principals, gültig bis, CA-Fingerprint, Aktionen "Zertifikat anhängen" (Einfügen oder Datei) und "Entfernen".
  - Öffentlichen Schlüssel kopieren.
- **Eintragen:**
  - "In authorized_keys eintragen" am Schlüssel (Host wählen) und im Kontextmenü eines Hosts (Schlüssel wählen, auch Agent-Schlüssel).
  - Ergebnis "eingetragen" oder "war schon eingetragen".
- **Anmeldedialog:** Bei `passphraseFor` lautet der Titel "Passphrase für Schlüssel „{name}“".
- **Statuszeile:** Hinweis "Zertifikat abgelaufen, angemeldet mit dem Schlüssel".

- [ ] **Step 1: Write the failing tests:**
  - `generateKeySpec` baut aus dem Formular die `KeySpec` und lehnt eine Passphrase-Wiederholung ab, die nicht passt.
  - `AuthPromptDialog` zeigt den Passphrase-Titel.
  - Jeder neue Schlüssel steht in `en.json` und `de.json` (vorhandener Vollständigkeitstest).
- [ ] **Step 2: Run, expect FAIL** (`npm test` in `app/`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests und Typecheck, expect PASS.** Danach `npm run tauri build`, den Installer in den Hauptordner kopieren und Moritz um die Prüfung von Hand bitten:
  - ECDSA- und RSA-Schlüssel erzeugen, mit und ohne gespeicherte Passphrase verbinden;
  - in `authorized_keys` eintragen;
  - Zertifikat anhängen und verbinden;
  - FIDO2-Token und Windows-Hello-Schlüssel erzeugen, eintragen und verbinden.
- [ ] **Step 5: Commit** `Oberfläche: Schlüssel erzeugen, Sicherheitsschlüssel, Zertifikate, Eintragen`

---

### Task 10: Doku und Fahrplan

**Files:**
- Create: `docs/keys.md`
- Modify: `docs/security.md`, `docs/sync.md`, `README.md` (Tabelle "Dokumentation"), `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md` (Zeile 10: "fertig, auf `main`")

**Inhalt von `docs/keys.md`** (Deutsch, kurz, für Benutzer):
- Schlüsseltypen;
- Passphrase speichern oder abfragen;
- Zertifikate (wo sie herkommen, was bei Ablauf passiert);
- in `authorized_keys` eintragen (was das Skript tut, nur Server mit `sh`);
- FIDO2 (OpenSSH ab 8.2 auf dem Server, `verify-required`);
- Windows Hello (gerätegebunden, nicht im Sync, nicht teilbar, beim Löschen weg).

`security.md`: Passphrase-Regeln, `KeyGate`-Regel, gerätegebundene Schlüssel. `sync.md`: Schlüssel tragen ihren gespeicherten Text und das Zertifikat, gerätegebundene nie.

- [ ] **Step 1:** Doku schreiben.
- [ ] **Step 2:** Volle Prüfung (kein CI im Repository):
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - in `app/`: `npm test`, `npm run typecheck`
  - Erwartet: alles grün.
- [ ] **Step 3: Commit** `Doku: Schlüssel, Fahrplan Teilplan 10 fertig`
