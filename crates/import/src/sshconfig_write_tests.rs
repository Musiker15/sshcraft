use super::*;

fn web() -> WriteHost {
    WriteHost {
        alias: "web".into(),
        hostname: Some("10.0.0.5".into()),
        port: Some(2222),
        user: Some("deploy".into()),
        jump: vec!["bastion".into()],
        proxy_command: None,
        forward_agent: Some(true),
    }
}

#[test]
fn write_back_keeps_other_lines() {
    let text = "# my config\nForwardAgent no\n\nHost other\n  HostName other.org # keep\n\nHost web\n  HostName old.org\n  IdentityFile ~/.ssh/web\n  User old\n  User older\n  ServerAliveInterval 30\n\nHost *\n  Compression yes\n";
    let out = write_host(text, &web()).unwrap();
    assert_eq!(
        out,
        "# my config\nForwardAgent no\n\nHost other\n  HostName other.org # keep\n\nHost web\n  HostName 10.0.0.5\n  Port 2222\n  User deploy\n  ProxyJump bastion\n  ForwardAgent yes\n  IdentityFile ~/.ssh/web\n  ServerAliveInterval 30\n\nHost *\n  Compression yes\n"
    );
}

#[test]
fn write_back_appends_a_new_host_before_the_first_block() {
    let text = "User global\n\nHost *\n  Compression yes\n";
    let out = write_host(text, &web()).unwrap();
    assert_eq!(
        out,
        "User global\n\nHost web\n    HostName 10.0.0.5\n    Port 2222\n    User deploy\n    ProxyJump bastion\n    ForwardAgent yes\n\nHost *\n  Compression yes\n",
        "before the first Host line, so its values win over Host *, and after the global lines"
    );
    assert_eq!(
        write_host("", &web()).unwrap(),
        "Host web\n    HostName 10.0.0.5\n    Port 2222\n    User deploy\n    ProxyJump bastion\n    ForwardAgent yes\n"
    );
}

#[test]
fn a_shared_host_line_is_not_edited_in_place() {
    // `Host web db` names two hosts: a new block for web goes first, the shared one stays.
    let text = "Host web db\n  User shared\n";
    let out = write_host(text, &web()).unwrap();
    assert!(out.starts_with("Host web\n    HostName 10.0.0.5\n"), "{out}");
    assert!(out.ends_with("Host web db\n  User shared\n"), "{out}");
}

#[test]
fn a_proxy_command_is_written_back_quoted() {
    let mut h = web();
    h.jump.clear();
    h.proxy_command = Some(vec!["ssh".into(), "-W".into(), "%h:%p".into(), "my bastion".into()]);
    let out = write_host("", &h).unwrap();
    assert!(out.contains("    ProxyCommand ssh -W %h:%p 'my bastion'\n"), "{out}");
    let parsed = crate::sshconfig::parse(&out);
    assert_eq!(parsed.hosts[0].proxy_command, h.proxy_command);
}

#[test]
fn values_that_would_break_the_file_are_refused() {
    for bad in ["a\nHost evil", "a\rb", "a\"b", ""] {
        let mut h = web();
        h.user = Some(bad.into());
        assert!(write_host("", &h).is_err(), "{bad:?}");
    }
    for alias in ["two words", "wild*", "!neg", "", "x\ny"] {
        let mut h = web();
        h.alias = alias.into();
        assert!(write_host("", &h).is_err(), "{alias:?}");
    }
    let mut h = web();
    h.user = Some("John Smith".into());
    assert!(write_host("", &h).unwrap().contains("    User \"John Smith\"\n"));
}

#[test]
fn crlf_files_stay_crlf() {
    let out = write_host("Host web\r\n  User old\r\n", &web()).unwrap();
    assert!(out.contains("  User deploy\r\n"), "{out:?}");
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
}
