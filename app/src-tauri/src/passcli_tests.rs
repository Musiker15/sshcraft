use std::sync::Mutex;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use sshcraft_core::store::Store;
use sshcraft_core::vault::{MemoryMasterKey, Vault};
use zeroize::Zeroizing;

use super::*;

const MARKER: &str = "MARKER-SECRET-4711";

const LIST_JSON: &str = r#"{
  "items": [
    {"id": "i1", "share_id": "s1", "vault_id": "v", "title": "server-root", "item_type": "ssh_key"},
    {"id": "i2", "share_id": "s1", "vault_id": "v", "title": "notes", "item_type": "custom"},
    {"id": "i3", "share_id": "s2", "vault_id": "v", "title": "deploy", "item_type": "ssh_key"}
  ],
  "folders": []
}"#;

fn view_json(private_key: &str) -> String {
    serde_json::json!({
        "item": {
            "id": "i1",
            "share_id": "s1",
            "content": {
                "title": "server-root",
                "content": { "SshKey": { "private_key": private_key, "public_key": "ssh-ed25519 AAAA" } }
            }
        }
    })
    .to_string()
}

struct FakePassCli {
    status: PassCliStatus,
    private_key: Mutex<String>,
}

impl FakePassCli {
    fn with_key(key: &str) -> Self {
        Self {
            status: PassCliStatus::Ready,
            private_key: Mutex::new(key.to_owned()),
        }
    }
}

impl PassCli for FakePassCli {
    fn status(&self) -> PassCliStatus {
        self.status
    }

    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError> {
        ensure_ready(self.status)?;
        parse_key_list(LIST_JSON)
    }

    fn private_key(
        &self,
        _share_id: &str,
        _item_id: &str,
    ) -> Result<Zeroizing<String>, PassCliError> {
        ensure_ready(self.status)?;
        parse_private_key(view_json(&self.private_key.lock().unwrap()).as_bytes())
    }
}

fn test_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn vault_and_store() -> (Vault, Mutex<Store>) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (vault, Mutex::new(store))
}

#[test]
fn key_list_keeps_only_ssh_keys() {
    let keys = parse_key_list(LIST_JSON).unwrap();
    let titles: Vec<_> = keys.iter().map(|k| k.title.as_str()).collect();
    assert_eq!(titles, vec!["server-root", "deploy"]);
    assert_eq!(keys[1].share_id, "s2");
    assert_eq!(keys[1].item_id, "i3");
}

#[test]
fn private_key_is_read_from_ssh_key_content() {
    let key = parse_private_key(view_json("PEM").as_bytes()).unwrap();
    assert_eq!(key.as_str(), "PEM");
}

#[test]
fn item_without_private_key_is_an_error() {
    let json = r#"{"item":{"content":{"content":{"Login":{"username":"x"}}}}}"#;
    assert!(matches!(
        parse_private_key(json.as_bytes()),
        Err(PassCliError::NoPrivateKey)
    ));
    assert!(matches!(
        parse_private_key(view_json("").as_bytes()),
        Err(PassCliError::NoPrivateKey)
    ));
}

#[test]
fn import_stores_the_key_encrypted() {
    let (vault, store) = vault_and_store();
    let key = test_key();
    let cli = FakePassCli::with_key(&key.to_openssh(LineEnding::LF).unwrap());
    let info = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "server-root").unwrap();
    assert_eq!(
        info.fingerprint,
        key.fingerprint(HashAlg::Sha256).to_string()
    );
    assert!(vault.private_key(&store.lock().unwrap(), info.id).is_ok());
}

#[test]
fn passphrase_protected_key_reports_encrypted() {
    let (vault, store) = vault_and_store();
    let encrypted = test_key().encrypt(&mut rand::rng(), "pw").unwrap();
    let cli = FakePassCli::with_key(&encrypted.to_openssh(LineEnding::LF).unwrap());
    let result = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "x");
    assert!(matches!(result, Err(PassCliError::Encrypted)));
}

#[test]
fn errors_never_contain_the_private_key() {
    let (vault, store) = vault_and_store();
    let broken = format!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n{MARKER}\n-----END OPENSSH PRIVATE KEY-----\n"
    );
    let cli = FakePassCli::with_key(&broken);
    let err = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "x").unwrap_err();
    assert!(!err.to_string().contains(MARKER), "{err}");
}

#[test]
fn process_failure_keeps_only_the_first_stderr_line() {
    let err = classify_failure(
        Some(1),
        b"Error: something went wrong
more detail
",
    );
    let text = err.to_string();
    assert!(text.contains("something went wrong"), "{text}");
    assert!(!text.contains("more detail"));
}

#[test]
fn classify_failure_only_reports_logged_out_for_login_messages() {
    assert!(matches!(
        classify_failure(
            Some(1),
            b"Error: not logged in
"
        ),
        PassCliError::LoggedOut
    ));
    assert!(matches!(
        classify_failure(Some(1), b"Please run `pass-cli login` first"),
        PassCliError::LoggedOut
    ));
    assert!(matches!(
        classify_failure(
            Some(1),
            b"error: network unreachable
"
        ),
        PassCliError::Failed(_)
    ));
    assert!(matches!(
        classify_failure(None, b""),
        PassCliError::Failed(_)
    ));
}

#[test]
fn run_with_timeout_kills_slow_process() {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-Command", "Start-Sleep 10"]);
        c
    } else {
        let mut c = std::process::Command::new("sleep");
        c.arg("10");
        c
    };
    command.stdin(std::process::Stdio::null());
    let started = std::time::Instant::now();
    let result = run_with_timeout(command, std::time::Duration::from_millis(300));
    assert!(matches!(result, Err(PassCliError::Timeout)), "{result:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn run_with_timeout_returns_output_of_fast_process() {
    let command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "echo hello"]);
        c
    } else {
        let mut c = std::process::Command::new("echo");
        c.arg("hello");
        c
    };
    let output = run_with_timeout(command, std::time::Duration::from_secs(10)).unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
}

#[test]
fn find_on_path_skips_relative_entries() {
    let exe = if cfg!(windows) {
        "pass-cli.exe"
    } else {
        "pass-cli"
    };
    let dir = std::env::temp_dir().join(format!("sshcraft-path-{}", std::process::id()));
    let relative = dir.join("rel");
    let empty = dir.join("abs");
    std::fs::create_dir_all(&relative).unwrap();
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(relative.join(exe), b"").unwrap();
    // "rel" only resolves relative to `dir`; a search must never look there.
    let path_var = std::env::join_paths([std::path::PathBuf::from("rel"), empty.clone()]).unwrap();
    let found = with_current_dir(&dir, || find_on_path(&path_var, exe));
    assert_eq!(found, None);
    let path_var = std::env::join_paths([relative.clone()]).unwrap();
    assert_eq!(find_on_path(&path_var, exe), Some(relative.join(exe)));
    std::fs::remove_dir_all(&dir).ok();
}

fn with_current_dir<T>(dir: &std::path::Path, f: impl FnOnce() -> T) -> T {
    let before = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = f();
    std::env::set_current_dir(before).unwrap();
    result
}

#[test]
fn logged_out_cli_reports_logged_out() {
    let cli = FakePassCli {
        status: PassCliStatus::LoggedOut,
        private_key: Mutex::new(String::new()),
    };
    assert!(matches!(cli.list_ssh_keys(), Err(PassCliError::LoggedOut)));
}

#[test]
fn private_key_with_escapes_is_decoded() {
    let key = parse_private_key(view_json("line1\nline2\n").as_bytes()).unwrap();
    assert_eq!(key.as_str(), "line1\nline2\n");
}

#[test]
fn run_with_timeout_does_not_wait_for_a_grandchild_holding_the_pipe() {
    // The child exits at once but leaves a grandchild that keeps stdout open.
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args([
            "/C",
            "start",
            "/b",
            "powershell",
            "-NoProfile",
            "-Command",
            "Start-Sleep 8",
        ]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", "sleep 8 &"]);
        c
    };
    command.stdin(std::process::Stdio::null());
    let started = std::time::Instant::now();
    let _ = run_with_timeout(command, std::time::Duration::from_millis(1500));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "took {:?}",
        started.elapsed()
    );
}
