use super::*;

const MOBA: &str = r#"[Misc]
SomeSetting=1

[Bookmarks]
SubRep=
ImgNum=42
top=#109#0%top.example.org%22%root%%-1%-1%%%%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%10#0# #-1

[Bookmarks_1]
SubRep=Kunden\Müller
ImgNum=41
web=#109#0%web.example.org%2222%deploy%%-1%-1%%%%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%10#0# #-1
desk=#91#4%desk.example.org%3389%admin%%-1#MobaFont%10#0# #-1
broken=#109#0%

[Passwords]
deploy@web.example.org=_SECRET_SHOULD_NEVER_APPEAR_
"#;

#[test]
fn mobaxterm_bookmarks_with_folders() {
    let plan = mobaxterm::parse(MOBA.as_bytes());
    let names: Vec<_> = plan.candidates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["top", "web"]);
    let web = &plan.candidates[1];
    assert_eq!(
        web.group_path,
        vec!["Kunden".to_owned(), "Müller".to_owned()]
    );
    assert_eq!(web.settings.address.as_deref(), Some("web.example.org"));
    assert_eq!(web.settings.port, Some(2222));
    assert_eq!(web.settings.username.as_deref(), Some("deploy"));
    assert!(plan.candidates[0].group_path.is_empty());
}

#[test]
fn mobaxterm_non_ssh_skipped() {
    let plan = mobaxterm::parse(MOBA.as_bytes());
    assert!(plan.candidates.iter().all(|c| c.name != "desk"));
    assert!(plan.skipped.iter().any(|s| s.contains("desk")));
    assert!(plan.skipped.iter().any(|s| s.contains("broken")));
}

#[test]
fn mobaxterm_passwords_are_not_imported() {
    let plan = mobaxterm::parse(MOBA.as_bytes());
    let all = serde_json::to_string(&plan).unwrap();
    assert!(!all.contains("_SECRET_SHOULD_NEVER_APPEAR_"));
    assert!(plan.skipped.iter().any(|s| s.contains("password")));
}

fn write(dir: &std::path::Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const CRT_WEB: &str = r#"S:"Protocol Name"=SSH2
S:"Hostname"=web.example.org
S:"Username"=deploy
D:"[SSH2] Port"=000008ae
S:"Identity Filename V2"=C:\keys\web.pub::rawkey
S:"Password V2"=02:_SECRET_SHOULD_NEVER_APPEAR_
"#;

#[test]
fn securecrt_tree_becomes_groups() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Kunden/Müller/web.ini", CRT_WEB);
    write(dir.path(), "Kunden/__FolderData__.ini", "S:\"Folder\"=x\n");
    write(dir.path(), "Default.ini", CRT_WEB);
    write(
        dir.path(),
        "telnet.ini",
        "S:\"Protocol Name\"=Telnet\nS:\"Hostname\"=old.example.org\n",
    );
    let plan = securecrt::parse_dir(dir.path());
    assert_eq!(plan.candidates.len(), 1, "{:?}", plan.skipped);
    let web = &plan.candidates[0];
    assert_eq!(web.name, "web");
    assert_eq!(
        web.group_path,
        vec!["Kunden".to_owned(), "Müller".to_owned()]
    );
    assert_eq!(
        web.key_note,
        Some(KeyNote::File {
            path: r"C:\keys\web.pub".into()
        })
    );
    assert!(web.warnings.contains(&ImportWarning::PasswordNotImported));
    assert!(!serde_json::to_string(&plan).unwrap().contains("_SECRET_"));
    assert_eq!(plan.skipped.len(), 1, "the telnet session");
}

#[test]
fn securecrt_hex_port() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "web.ini", CRT_WEB);
    let plan = securecrt::parse_dir(dir.path());
    assert_eq!(plan.candidates[0].settings.port, Some(2222));
}

#[test]
fn truncated_files_do_not_abort() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.ini", "S:\"Protocol Name\"=SSH2\nS:\"Host");
    write(dir.path(), "b.ini", CRT_WEB);
    std::fs::write(dir.path().join("c.ini"), [0xff, 0xfe, 0x00, 0xd8]).unwrap();
    let plan = securecrt::parse_dir(dir.path());
    assert_eq!(plan.candidates.len(), 1);
    let moba = mobaxterm::parse(b"[Bookmarks]\nSubRep=\nx=#109#0%h");
    assert!(moba.candidates.is_empty());
}
