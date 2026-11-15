use super::*;

fn s(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn quotes_are_split_like_ssh() {
    assert_eq!(split("ssh -W %h:%p bastion").unwrap(), s(&["ssh", "-W", "%h:%p", "bastion"]));
    assert_eq!(split(r#"  "C:\Program Files\nc.exe"  'a b'  c\ d "x\"y" "#).unwrap(), s(&[r"C:\Program Files\nc.exe", "a b", "c d", "x\"y"]));
    assert_eq!(split("exec nc %h %p").unwrap(), s(&["nc", "%h", "%p"]), "a leading exec is dropped");
    assert_eq!(split("''").unwrap(), s(&[""]));
}

#[test]
fn shell_syntax_is_refused() {
    for line in [
        "nc %h %p | tee x",
        "nc %h %p; rm -rf /",
        "nc %h %p && x",
        "nc $HOST %p",
        "nc \"$HOST\" %p",
        "nc `id` %p",
        "nc %h %p > /tmp/x",
        "(nc %h %p)",
        "nc %h %p &",
    ] {
        assert!(matches!(split(line), Err(ProxyCommandError::ShellSyntax(_))), "{line}");
    }
    // Inside single quotes nothing is special to sh, so nothing is refused.
    assert_eq!(split("echo 'a|b;$c'").unwrap(), s(&["echo", "a|b;$c"]));
}

#[test]
fn broken_lines_are_refused() {
    assert_eq!(split("nc 'open"), Err(ProxyCommandError::Unclosed));
    assert_eq!(split("nc \"open"), Err(ProxyCommandError::Unclosed));
    assert_eq!(split("nc \\"), Err(ProxyCommandError::Unclosed));
    assert_eq!(split("   "), Err(ProxyCommandError::Empty));
    assert_eq!(split("exec"), Err(ProxyCommandError::Empty));
    assert_eq!(split("nc\u{0}x"), Err(ProxyCommandError::ShellSyntax('\u{0}')));
}

#[test]
fn tokens_stay_one_argument() {
    let argv = s(&["nc", "%h", "%p", "--user=%r", "%n"]);
    let out = expand(&argv, &Tokens { host: "a b;rm x", port: 2222, user: "$(id)", alias: "web" });
    assert_eq!(out, s(&["nc", "a b;rm x", "2222", "--user=$(id)", "web"]));
}

#[test]
fn percent_percent_is_a_percent() {
    let out = expand(&s(&["x", "100%%", "%q", "%"]), &Tokens { host: "h", port: 22, user: "u", alias: "a" });
    assert_eq!(out, s(&["x", "100%", "%q", "%"]), "unknown tokens stay as they are");
}

#[test]
fn a_command_joins_back_for_display() {
    assert_eq!(join(&s(&["ssh", "-W", "%h:%p", "my host", "it's"])), r#"ssh -W %h:%p 'my host' 'it'\''s'"#);
    assert_eq!(split(&join(&s(&["a b", "it's", "", "x|y"]))).unwrap(), s(&["a b", "it's", "", "x|y"]));
}
