# SSHCraft Teilplan 3: Protokoll- und Krypto-Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eine reine Rust-Bibliothek `sshcraft-proto`, die Konto- und Geräteschlüssel, das signierte Geräteprotokoll, Bereichsschlüssel, verschlüsselte und signierte Datensätze und den Schutz gegen Rückfall auf alte Stände umsetzt, so dass ein übernommener Sync-Server weder mitlesen noch unbemerkt etwas unterschieben kann.

**Architecture:** Ohne Ein- und Ausgabe, ohne Datenbank, ohne Netz: nur Typen, die sich serialisieren lassen, und Funktionen, die prüfen. Client (Teilplan 5 und 6) und Server (Teilplan 4) nutzen dieselben Typen. Der Server sieht nur Kopfdaten (IDs, Versionen, öffentliche Schlüssel), die Inhalte nur als Chiffrat. `sshcraft-proto` hängt nicht von `sshcraft-core` ab, damit der Server ohne SQLite und Tresor auskommt; der Inhalt eines Datensatzes ist für die Bibliothek nur Bytes.

**Tech Stack:** Rust 1.99, ed25519-dalek 3.0.0 (Signaturen), hpke 0.15.1 mit `default-features = false, features = ["alloc", "getrandom", "x25519", "chacha"]` (RFC 9180, Suite DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20Poly1305, Modus Base), chacha20poly1305 0.11.0 (XChaCha20-Poly1305 für Datensätze), sha2 0.11.1, data-encoding 2.11.1 (Base32 und Base64), rand 0.10.3, uuid, serde, serde_json, thiserror, zeroize (Feature `serde`).

**Spec:** `docs/superpowers/specs/2026-11-11-sshcraft-design.md`, Abschnitte 6 (Konten, Geräte, Geräte-Sperre) und 7 (Ende-zu-Ende-Verschlüsselung, Angreifermodell), Fahrplan `docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md`.

## Global Constraints

- Alle Regeln aus den Teilplänen 1 und 2 gelten weiter (Arbeitsbäume unter `G:\Claude\worktrees`, Code Englisch, Commits Deutsch ohne KI-Zuschreibung und ohne Gedankenstriche, UTF-8, neueste stabile Versionen, Gegenproben für Regeln, Rechte und Datenwege).
- **Keine eigene Kryptografie:** nur Ed25519, HPKE nach RFC 9180 und XChaCha20-Poly1305 aus den genannten Crates. Selbst gebaut werden nur Datenformate.
- **Domänentrennung:** Jede Signatur geht über `signing_bytes(domain, fields)` mit einem eigenen festen Domänen-String je Objektart (Liste in Task 1). Eine Signatur für eine Objektart darf nie für eine andere gelten.
- Geheime Schlüssel (`AccountKey`, `DeviceKeys`, `AreaKey`) implementieren weder `Debug` mit Inhalt noch `Serialize`; ihre Bytes gibt es nur als `Zeroizing`. Öffentliche Teile dürfen serialisiert werden.
- Zeitstempel als Unix-Sekunden `i64`, vom Aufrufer übergeben; die Bibliothek liest keine Uhr.
- Binäre Felder in JSON als Base64 ohne Polsterung (`data_encoding::BASE64URL_NOPAD`).
- **[Claudes Entscheidung]** Das Konto hat nur einen Signaturschlüssel (Ed25519). Verschlüsselt wird an die Geräte (je ein HPKE-X25519-Schlüssel), wie im Design "Die Verschlüsselung für Freigaben richtet sich an die Geräte eines Kontos". Der Kontoschlüssel unterschreibt Geräte-Zertifikate und kann Geräte sperren.

## Review Focus

1. **Kontoschlüssel aus Proton Pass eingefügt** mit Zeilenumbrüchen, Leerzeichen, anderer Groß- und Kleinschreibung: wird erkannt; ein einziges falsches Zeichen ergibt "Prüfsumme stimmt nicht", nie einen anderen, gültig aussehenden Schlüssel. Tests in Task 2.
2. **Geräteprotokoll vom Server manipuliert** (Eintrag gefälscht, wiederholt, umsortiert, gekürzt; ein gesperrtes Gerät sperrt ein anderes; zwei Geräte sperren sich gegenseitig): wird abgelehnt oder eindeutig aufgelöst. Tests in Task 3.
3. **Datensatz umgehängt** (anderer Bereich, andere Datensatz-ID, andere Version, anderer Schlüsselstand): wird abgelehnt. Tests in Task 5.
4. **Datensatz von einem Mitglied mit Leserecht oder von einem gesperrten Gerät:** wird abgelehnt. Tests in Task 5.
5. **Älterer Stand oder zwei verschiedene Inhalte mit derselben Version:** wird als Rückfall bzw. Gabelung abgelehnt. Tests in Task 5.

---

## Dateistruktur

```
crates/sshcraft-proto/
  Cargo.toml
  src/lib.rs          Module, ProtoError
  src/encoding.rs     signing_bytes(), Domain-Konstanten, b64-serde-Hilfe
  src/identity.rs     AccountKey, AccountPublic, safety_number(), DeviceKeys, DevicePublic, DeviceCertificate
  src/devicelog.rs    LogEntry, LogAction, LogSigner, DeviceLog, DeviceSet, LogHead
  src/area.rs         AreaId, AreaKey, KeyEnvelope, AreaManifest, Member, Role
  src/record.rs       SealedRecord, RecordHeader, VersionTracker, Freshness
  tests/*.rs          je Modul eine Testdatei
docs/protocol.md      Formate und Prüfregeln für Leute, die einen eigenen Client oder Server bauen (Englisch)
```

---

### Task 1: Crate-Gerüst, Kodierung und Fehler

**Files:**
- Create: `crates/sshcraft-proto/Cargo.toml`, `src/lib.rs`, `src/encoding.rs`, `tests/encoding.rs`
- Modify: `Cargo.toml` (Workspace-Mitglied, neue Abhängigkeiten unter `[workspace.dependencies]`)

**Interfaces:**
- Produces:
  - `pub enum ProtoError` (`thiserror`): `BadAccountKeyFormat`, `BadChecksum`, `BadSignature`, `UnknownDevice`, `DeviceRevoked`, `LogBroken(String)`, `Rollback`, `Fork`, `NotAllowed`, `Decrypt`, `Malformed(String)`.
  - `pub(crate) fn signing_bytes(domain: &str, fields: &[&[u8]]) -> Vec<u8>`: `domain` als UTF-8, dann je Feld `u32` Länge (Big Endian) und Bytes. Damit ist die Zuordnung eindeutig, auch bei leeren Feldern.
  - Domänen-Konstanten: `DOMAIN_DEVICE_CERT = "sshcraft/device-cert/v1"`, `DOMAIN_LOG_ENTRY = "sshcraft/device-log/v1"`, `DOMAIN_MANIFEST = "sshcraft/area-manifest/v1"`, `DOMAIN_RECORD = "sshcraft/record/v1"`, `DOMAIN_RECORD_AAD = "sshcraft/record-aad/v1"`, `DOMAIN_ENVELOPE_INFO = "sshcraft/area-key/v1"`, `DOMAIN_SAFETY = "sshcraft/safety-number/v1"`, `DOMAIN_LOG_HASH = "sshcraft/device-log-hash/v1"`, `DOMAIN_AUTH = "sshcraft/server-auth/v1"`.
  - `pub mod b64`: serde-`with`-Modul für `Vec<u8>` und `[u8; N]` als Base64 ohne Polsterung.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn field_boundaries_are_unambiguous() { /* signing_bytes(d, &[b"ab", b"c"]) != signing_bytes(d, &[b"a", b"bc"]) */ }
#[test] fn empty_fields_are_distinct() { /* &[b"", b"x"] != &[b"x", b""] and != &[b"x"] */ }
#[test] fn domain_separates() { /* same fields, other domain -> different bytes */ }
#[test] fn b64_roundtrip_in_json() { /* struct with #[serde(with = "b64")] field -> JSON string without '=' -> same bytes back */ }
```

`signing_bytes` ist `pub(crate)`; die Tests dafür stehen deshalb als `#[cfg(test)]`-Modul in `encoding.rs`, nur der b64-Test in `tests/encoding.rs`.

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen; aktuelle Versionen prüfen und in `[workspace.dependencies]` eintragen.
- [ ] **Step 4:** `cargo test -p sshcraft-proto` grün.
- [ ] **Step 5:** Commit `sshcraft-proto: Gerüst, kanonische Signaturbytes, Base64`.

---

### Task 2: Kontoschlüssel und Geräte

**Files:**
- Create: `crates/sshcraft-proto/src/identity.rs`, `tests/identity.rs`

**Interfaces:**
- Produces:
  - `pub struct AccountKey` (Ed25519-Signaturschlüssel, kein `Debug`-Inhalt): `generate() -> Self`, `public(&self) -> AccountPublic`, `export(&self) -> Zeroizing<String>`, `import(text: &str) -> Result<Self, ProtoError>`.
  - **Exportformat** `sshcraft-account-1:` + Base32 (RFC 4648, Großbuchstaben, ohne Polsterung) von `seed (32 Bytes) || SHA-256(seed)[0..4]`. Beim Import: alle Leerzeichen und Zeilenumbrüche entfernen, Groß- und Kleinschreibung egal; falscher Präfix oder falsche Länge → `BadAccountKeyFormat`, falsche Prüfsumme → `BadChecksum`.
  - `pub struct AccountPublic([u8; 32])` (`Serialize`, `Deserialize` über `b64`, `Eq`, `Hash`, `Copy`): `verify(&self, msg: &[u8], sig: &[u8; 64]) -> Result<(), ProtoError>`.
  - `pub fn safety_number(account: &AccountPublic) -> String`: `SHA-256(DOMAIN_SAFETY || bytes)`, die ersten 30 Bytes in sechs Blöcke zu je 5 Bytes, jeder Block als Big-Endian-Zahl modulo 100000, fünfstellig mit führenden Nullen, durch Leerzeichen getrennt (Beispiel `"04213 99120 00017 53381 72004 10855"`).
  - `pub struct DeviceKeys` (Ed25519-Signaturschlüssel und HPKE-X25519-Schlüssel): `generate() -> Self`, `public(&self) -> DevicePublic`, `to_bytes(&self) -> Zeroizing<[u8; 64]>`, `from_bytes(&[u8; 64]) -> Result<Self, ProtoError>`, `sign(&self, msg: &[u8]) -> [u8; 64]` (crate-intern).
  - `pub struct DevicePublic { pub signing: [u8; 32], pub hpke: [u8; 32] }` (serialisierbar).
  - Anmeldung am Server (für Teilplan 4): `DeviceKeys::sign_auth(&self, server_challenge: &[u8], device_id: Uuid) -> [u8; 64]` über `signing_bytes(DOMAIN_AUTH, [server_challenge, device_id])`; `DeviceCertificate::verify_auth(&self, server_challenge: &[u8], signature: &[u8; 64]) -> Result<(), ProtoError>`.
  - `pub struct DeviceCertificate { pub device_id: Uuid, pub account: AccountPublic, pub keys: DevicePublic, pub name: String, pub platform: String, pub created_at: i64, pub signature: [u8; 64] }` (serialisierbar): `issue(account: &AccountKey, device_id: Uuid, keys: DevicePublic, name: &str, platform: &str, created_at: i64) -> Self`, `verify(&self) -> Result<(), ProtoError>` (Signatur des eigenen `account` über `signing_bytes(DOMAIN_DEVICE_CERT, [device_id, account, keys.signing, keys.hpke, name, platform, created_at])`).

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn account_key_export_import_roundtrip() {}
#[test] fn import_tolerates_whitespace_newlines_and_case() { /* export, insert "\n", "  ", lowercase -> same public key */ }
#[test] fn one_changed_character_fails_the_checksum() { /* flip one Base32 char in the body -> Err(BadChecksum), for every position */ }
#[test] fn wrong_prefix_or_length_is_a_format_error() {}
#[test] fn safety_number_is_stable_and_formatted() { /* same key -> same string; 6 groups of 5 digits */ }
#[test] fn different_keys_have_different_safety_numbers() {}
#[test] fn device_keys_roundtrip_bytes() {}
#[test] fn issued_certificate_verifies() {}
#[test] fn certificate_with_changed_field_fails() { /* change name, platform, created_at, hpke key one at a time -> BadSignature */ }
#[test] fn certificate_signed_by_other_account_fails() { /* issue with A, set account = B.public() -> BadSignature */ }
#[test] fn auth_signature_verifies_only_for_its_challenge_and_device() { /* other challenge or other device's cert -> BadSignature */ }
#[test] fn auth_signature_is_not_a_record_or_log_signature() { /* same bytes signed under DOMAIN_RECORD do not verify as auth */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenproben:** Prüfsumme beim Import nicht prüfen → `one_changed_character_fails_the_checksum` rot. In `verify` das Feld `name` nicht mitsignieren → `certificate_with_changed_field_fails` rot.
- [ ] **Step 6:** Commit `sshcraft-proto: Kontoschlüssel, Sicherheitsnummer, Geräte-Zertifikate`.

---

### Task 3: Geräteprotokoll

**Files:**
- Create: `crates/sshcraft-proto/src/devicelog.rs`, `tests/devicelog.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces:
  - `pub enum LogAction { Add(DeviceCertificate), Revoke { device_id: Uuid } }`
  - `pub enum LogSigner { Account, Device(Uuid) }`
  - `pub struct LogEntry { pub seq: u64, pub prev_hash: [u8; 32], pub action: LogAction, pub signer: LogSigner, pub created_at: i64, pub signature: [u8; 64] }` (serialisierbar); `hash(&self) -> [u8; 32]` = `SHA-256(signing_bytes(DOMAIN_LOG_HASH, [seq, prev_hash, kanonische Bytes der Aktion, signer, created_at, signature]))`.
  - `pub struct LogHead { pub seq: u64, pub hash: [u8; 32] }` (serialisierbar).
  - `pub struct DeviceSet` mit `active(&self) -> impl Iterator<Item = &DeviceCertificate>`, `get_active(&self, id: Uuid) -> Option<&DeviceCertificate>`, `head(&self) -> LogHead`.
  - `pub struct DeviceLog { pub entries: Vec<LogEntry> }` mit:
    - `append_add(&mut self, account: &AccountKey, cert: DeviceCertificate, created_at: i64)` (vom Kontoschlüssel signiert, beim Einrichten eines Geräts)
    - `append_revoke_by_device(&mut self, device_id: Uuid, keys: &DeviceKeys, target: Uuid, created_at: i64)`
    - `append_revoke_by_account(&mut self, account: &AccountKey, target: Uuid, created_at: i64)`
    - `verify(&self, account: &AccountPublic) -> Result<DeviceSet, ProtoError>`
    - `verify_extends(&self, account: &AccountPublic, known: Option<&LogHead>) -> Result<DeviceSet, ProtoError>`: wie `verify`, zusätzlich muss der Eintrag an `known.seq` den Hash `known.hash` haben, sonst `Rollback` (Protokoll kürzer) bzw. `Fork` (anderer Eintrag).
- **Regeln in `verify`:** `seq` beginnt bei 0 und ist lückenlos; `prev_hash` des ersten Eintrags sind 32 Nullbytes, danach der Hash des Vorgängers; `Add` nur vom Kontoschlüssel signiert, Zertifikat gültig, `cert.account == account`, `device_id` noch nicht vorhanden; `Revoke` vom Kontoschlüssel oder von einem Gerät, das **zu diesem Zeitpunkt** aktiv ist; das Ziel muss existieren und aktiv sein. Ein gesperrtes Gerät kann danach nichts mehr signieren. Damit lösen sich gegenseitige Sperren in Protokollreihenfolge auf: wer zuerst sperrt, gewinnt, der zweite Eintrag ist ungültig und das ganze Protokoll wird abgelehnt (`LogBroken`), so dass der Server es nicht stillschweigend übernehmen kann.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn two_devices_added_by_account_are_active() {}
#[test] fn device_can_revoke_another_device() {}
#[test] fn account_can_revoke_when_no_device_is_left() {}
#[test] fn add_signed_by_a_device_is_rejected() {}
#[test] fn add_of_a_certificate_for_another_account_is_rejected() {}
#[test] fn revoked_device_cannot_sign_afterwards() { /* A revokes B, then B revokes A -> Err(LogBroken) */ }
#[test] fn forged_entry_signature_is_rejected() {}
#[test] fn reordered_or_gapped_entries_are_rejected() { /* swap two entries; remove a middle one -> Err(LogBroken) */ }
#[test] fn changed_prev_hash_is_rejected() {}
#[test] fn shorter_log_than_known_is_a_rollback() { /* known head at seq 3, log has 2 entries -> Err(Rollback) */ }
#[test] fn different_entry_at_known_seq_is_a_fork() {}
#[test] fn longer_log_extending_known_head_is_accepted() {}
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenproben:** Prüfung "Signierer ist zu diesem Zeitpunkt aktiv" entfernen → `revoked_device_cannot_sign_afterwards` rot. Prüfung von `prev_hash` entfernen → `changed_prev_hash_is_rejected` rot. Vergleich mit `known.hash` entfernen → `different_entry_at_known_seq_is_a_fork` rot.
- [ ] **Step 6:** Commit `sshcraft-proto: signiertes Geräteprotokoll mit Sperren`.

---

### Task 4: Bereichsschlüssel und Umschläge

**Files:**
- Create: `crates/sshcraft-proto/src/area.rs` (erster Teil), `tests/envelope.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces:
  - `pub struct AreaId(pub Uuid)` (serialisierbar, `Copy`, `Eq`, `Hash`).
  - `pub struct AreaKey` (32 Bytes, XChaCha20-Poly1305-Schlüssel, kein `Debug`-Inhalt): `generate() -> Self`.
  - `pub struct KeyEnvelope { pub area_id: AreaId, pub key_version: u32, pub device_id: Uuid, pub encapped: Vec<u8>, pub ciphertext: Vec<u8> }` (serialisierbar, Bytes über `b64`).
  - `pub fn seal_area_key(key: &AreaKey, area_id: AreaId, key_version: u32, recipient: &DeviceCertificate) -> Result<KeyEnvelope, ProtoError>`: HPKE Single-Shot-Seal im Modus Base an `recipient.keys.hpke`, `info = DOMAIN_ENVELOPE_INFO`, `aad = signing_bytes(DOMAIN_ENVELOPE_INFO, [area_id, key_version, device_id])`.
  - `pub fn open_area_key(envelope: &KeyEnvelope, device_id: Uuid, keys: &DeviceKeys) -> Result<AreaKey, ProtoError>`: `envelope.device_id != device_id` → `NotAllowed`; Entschlüsselungsfehler → `Decrypt`.

- [ ] **Step 1: Failing tests:**

```rust
#[test] fn sealed_area_key_opens_for_its_device() {}
#[test] fn other_device_cannot_open() { /* open with keys of device B -> Err(Decrypt) or Err(NotAllowed) */ }
#[test] fn envelope_moved_to_other_area_or_version_fails() { /* change area_id or key_version -> Err(Decrypt) */ }
#[test] fn envelope_relabelled_for_other_device_fails() { /* set device_id = B, open with B's id and keys -> Err(Decrypt) */ }
#[test] fn two_seals_of_the_same_key_differ() { /* fresh encapsulation each time */ }
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenprobe:** `aad` ohne `area_id` bilden → `envelope_moved_to_other_area_or_version_fails` rot.
- [ ] **Step 6:** Commit `sshcraft-proto: Bereichsschlüssel per HPKE an Geräte`.

---

### Task 5: Bereichsbeschreibung, Datensätze und Rückfallschutz

**Files:**
- Modify: `crates/sshcraft-proto/src/area.rs`
- Create: `crates/sshcraft-proto/src/record.rs`, `tests/manifest.rs`, `tests/record.rs`

**Interfaces:**
- Consumes: Tasks 2 bis 4.
- Produces:
  - `pub enum Role { Read, Write }`, `pub struct Member { pub account: AccountPublic, pub role: Role }`.
  - `pub struct AreaManifest { pub area_id: AreaId, pub version: u64, pub key_version: u32, pub owner: AccountPublic, pub members: Vec<Member>, pub author_device: Uuid, pub signature: [u8; 64] }` (serialisierbar):
    - `sign(area_id, version, key_version, owner: AccountPublic, members, author_device: Uuid, keys: &DeviceKeys) -> Self`
    - `verify(&self, owner_devices: &DeviceSet) -> Result<(), ProtoError>`: `author_device` muss in `owner_devices` aktiv sein (`DeviceRevoked`/`UnknownDevice`), Signatur gültig; `owner_devices` gehört zum Konto `owner` (Prüfung über die Zertifikate).
    - `can_write(&self, account: &AccountPublic) -> bool`: Eigentümer oder Mitglied mit `Write`. Ein persönlicher Bereich hat keine Mitglieder.
  - `pub struct RecordHeader { pub record_id: Uuid, pub area_id: AreaId, pub version: u64, pub key_version: u32, pub author_account: AccountPublic, pub author_device: Uuid }` (serialisierbar, für den Server lesbar).
  - `pub struct SealedRecord { pub header: RecordHeader, pub nonce: [u8; 24], pub ciphertext: Vec<u8>, pub signature: [u8; 64] }` (serialisierbar).
  - `SealedRecord::seal(header: RecordHeader, body: &[u8], key: &AreaKey, keys: &DeviceKeys) -> Self`: AEAD mit `aad = signing_bytes(DOMAIN_RECORD_AAD, [alle Header-Felder])`, Signatur über `signing_bytes(DOMAIN_RECORD, [aad, nonce, ciphertext])`.
  - `SealedRecord::open(&self, key: &AreaKey, manifest: &AreaManifest, author_devices: &DeviceSet) -> Result<Zeroizing<Vec<u8>>, ProtoError>`: Reihenfolge der Prüfungen: `header.area_id == manifest.area_id` und `header.key_version == manifest.key_version` (sonst `NotAllowed`); `manifest.can_write(author_account)` (sonst `NotAllowed`); `author_device` in `author_devices` aktiv und dessen Zertifikat gehört zu `author_account` (sonst `DeviceRevoked`/`UnknownDevice`); Signatur (sonst `BadSignature`); dann entschlüsseln (sonst `Decrypt`).
  - `pub enum Freshness { New, Unchanged }`
  - `pub struct VersionTracker` (serialisierbar, der Client speichert ihn): `check(&mut self, id: Uuid, version: u64, digest: [u8; 32]) -> Result<Freshness, ProtoError>`: höhere Version → merken, `New`; gleiche Version und gleicher Digest → `Unchanged`; gleiche Version, anderer Digest → `Fork`; niedrigere Version → `Rollback`. Der Digest ist `SHA-256` der Signatur des Datensatzes (`SealedRecord::digest()`); für Bereichsbeschreibungen `AreaManifest::digest()` entsprechend.

- [ ] **Step 1: Failing tests** in `tests/manifest.rs` und `tests/record.rs` (Hilfsfunktion baut Konto, zwei Geräte, Protokoll, persönlichen Bereich und einen geteilten Bereich mit einem Leser und einem Schreiber):

```rust
#[test] fn manifest_signed_by_owner_device_verifies() {}
#[test] fn manifest_signed_by_revoked_device_is_rejected() {}
#[test] fn manifest_with_changed_member_role_is_rejected() {}
#[test] fn owner_record_roundtrips() {}
#[test] fn writer_member_record_is_accepted() {}
#[test] fn reader_member_record_is_rejected() { /* Err(NotAllowed) */ }
#[test] fn stranger_record_is_rejected() {}
#[test] fn record_from_revoked_device_is_rejected() {}
#[test] fn record_claiming_another_account_is_rejected() { /* author_account = owner, author_device = stranger's device */ }
#[test] fn header_tampering_is_rejected() { /* change record_id, area_id, version, key_version one at a time -> rejected */ }
#[test] fn ciphertext_tampering_is_rejected() {}
#[test] fn signature_of_one_record_does_not_fit_another() {}
#[test] fn tracker_accepts_newer_and_unchanged() {}
#[test] fn tracker_rejects_rollback() {}
#[test] fn tracker_rejects_fork() {}
#[test] fn tracker_survives_serialization() {}
```

- [ ] **Step 2:** Tests schlagen fehl.
- [ ] **Step 3:** Umsetzen.
- [ ] **Step 4:** Tests grün.
- [ ] **Step 5: Gegenproben:** `can_write` immer `true` → `reader_member_record_is_rejected` rot. Prüfung "Gerät gehört zu `author_account`" entfernen → `record_claiming_another_account_is_rejected` rot. `version` aus dem `aad` nehmen → `header_tampering_is_rejected` rot. `Fork`-Fall als `Unchanged` behandeln → `tracker_rejects_fork` rot.
- [ ] **Step 6:** Commit `sshcraft-proto: Bereichsbeschreibung, signierte Datensätze, Rückfallschutz`.

---

### Task 6: Protokollbeschreibung

**Files:**
- Create: `docs/protocol.md` (Englisch)
- Modify: `README.md` (Tabelle "Documentation": Zeile "Protocol and cryptography")

- [ ] **Step 1:** `docs/protocol.md` beschreibt für Leute, die einen eigenen Client oder Server schreiben: Schlüsselarten und wer was signiert; Exportformat des Kontoschlüssels; Sicherheitsnummer; `signing_bytes` mit allen Domänen; Geräteprotokoll mit Prüfregeln; HPKE-Suite und `aad` der Umschläge; Datensatz-Kopf, `aad`, Signatur und Reihenfolge der Prüfungen; Rückfallschutz; was der Server sieht und was nicht; offene Grenzen (Metadaten wie Anzahl, Größe und Zeitpunkt von Datensätzen sind für den Server sichtbar).
- [ ] **Step 2:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` grün.
- [ ] **Step 3:** Commit `Protokollbeschreibung für sshcraft-proto`.
