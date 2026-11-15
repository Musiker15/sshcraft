//! The token file (made once, kept private, reused) and the connection file (no secret in it).

use sshcraft_link::{CONNECTION_FILE, ConnectionFile, TOKEN_FILE, Token};

#[test]
fn the_token_file_is_reused_and_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(TOKEN_FILE);
    let first = Token::load_or_create(&path).unwrap();
    let second = Token::load_or_create(&path).unwrap();
    assert_eq!(first.hex(), second.hex(), "the same token again");
    assert_eq!(first.hex().len(), 64);
    assert!(first.hex().chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(Token::generate().hex(), Token::generate().hex());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "only the owner reads it");
    }
    std::fs::write(&path, "not a token").unwrap();
    assert!(Token::load_or_create(&path).is_err(), "a damaged token file is not silently replaced");
}

#[test]
fn a_connection_file_round_trips_and_a_bad_one_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(ConnectionFile::read(dir.path()).is_none());
    let file = ConnectionFile::new(4321, dir.path().join(TOKEN_FILE), "0.1.0".into());
    file.write(dir.path()).unwrap();
    let read = ConnectionFile::read(dir.path()).unwrap();
    assert_eq!(read, file);
    let text = std::fs::read_to_string(dir.path().join(CONNECTION_FILE)).unwrap();
    assert!(text.contains("\"tokenFile\"") && text.contains("\"version\": 1"), "{text}");
    std::fs::write(dir.path().join(CONNECTION_FILE), r#"{"version": 2, "pid": 1, "port": 1, "tokenFile": "x", "app": ""}"#).unwrap();
    assert!(ConnectionFile::read(dir.path()).is_none(), "another version");
    std::fs::write(dir.path().join(CONNECTION_FILE), "garbage").unwrap();
    assert!(ConnectionFile::read(dir.path()).is_none());
    ConnectionFile::remove(dir.path()).unwrap();
    ConnectionFile::remove(dir.path()).unwrap();
    assert!(!dir.path().join(CONNECTION_FILE).exists());
}
