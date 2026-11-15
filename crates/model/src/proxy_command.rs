//! A proxy command (OpenSSH `ProxyCommand`) as an argument list. OpenSSH hands the line to a
//! shell; SSHCraft never does (AGENTS.md, "No shell string from foreign data"). The line is
//! split once, by the quoting rules of `sh`, and refused when it needs anything only a shell can
//! do: pipes, lists, redirection, substitution, variables. The tokens `%h`, `%p`, `%r`, `%n` are
//! then replaced inside each argument, so a host name with spaces or `;` stays one argument.

/// The longest proxy command line accepted.
pub const MAX_LINE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProxyCommandError {
    #[error("the proxy command is empty")]
    Empty,
    #[error("the proxy command is too long")]
    TooLong,
    #[error("a quote or a backslash is not closed")]
    Unclosed,
    /// The character only means something to a shell (or is a control character).
    #[error("the proxy command needs a shell for '{0}'; SSHCraft runs it without one")]
    ShellSyntax(char),
}

/// Characters a shell would act on outside quotes.
fn shell_only(c: char) -> bool {
    matches!(c, '|' | ';' | '&' | '<' | '>' | '(' | ')' | '$' | '`')
}

/// What a backslash outside quotes escapes. Any other backslash is kept, so a Windows path
/// such as `C:\tools\nc.exe` stays as written.
fn escapable(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\'' | '"' | '\\') || shell_only(c)
}

/// Splits a proxy command line into its arguments, as `sh` would without running anything.
pub fn split(line: &str) -> Result<Vec<String>, ProxyCommandError> {
    if line.len() > MAX_LINE {
        return Err(ProxyCommandError::TooLong);
    }
    if let Some(c) = line.chars().find(|c| c.is_control() && !matches!(c, ' ' | '\t')) {
        return Err(ProxyCommandError::ShellSyntax(c));
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(ProxyCommandError::Unclosed),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c @ ('$' | '`')) => return Err(ProxyCommandError::ShellSyntax(c)),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(ProxyCommandError::Unclosed),
                        },
                        Some(c @ ('$' | '`')) => return Err(ProxyCommandError::ShellSyntax(c)),
                        Some(c) => word.push(c),
                        None => return Err(ProxyCommandError::Unclosed),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.peek().copied() {
                    None => return Err(ProxyCommandError::Unclosed),
                    Some(next) if escapable(next) => {
                        word.push(next);
                        chars.next();
                    }
                    Some(_) => word.push('\\'),
                }
            }
            c if shell_only(c) => return Err(ProxyCommandError::ShellSyntax(c)),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    // `ProxyCommand exec nc %h %p` is a common way to save a shell process; without a shell
    // the word means nothing.
    if words.first().is_some_and(|w| w == "exec") {
        words.remove(0);
    }
    if words.is_empty() {
        return Err(ProxyCommandError::Empty);
    }
    Ok(words)
}

/// The values the tokens stand for.
#[derive(Debug, Clone, Copy)]
pub struct Tokens<'a> {
    /// `%h`: the address the proxy connects to.
    pub host: &'a str,
    /// `%p`
    pub port: u16,
    /// `%r`: the user name.
    pub user: &'a str,
    /// `%n`: the host's name in SSHCraft (the alias in `~/.ssh/config`).
    pub alias: &'a str,
}

/// Replaces `%h`, `%p`, `%r`, `%n` and `%%` in each argument. Unknown tokens stay as written.
pub fn expand(argv: &[String], tokens: &Tokens<'_>) -> Vec<String> {
    argv.iter()
        .map(|arg| {
            let mut out = String::with_capacity(arg.len());
            let mut chars = arg.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '%' {
                    out.push(c);
                    continue;
                }
                match chars.peek().copied() {
                    Some('h') => out.push_str(tokens.host),
                    Some('p') => out.push_str(&tokens.port.to_string()),
                    Some('r') => out.push_str(tokens.user),
                    Some('n') => out.push_str(tokens.alias),
                    Some('%') => out.push('%'),
                    _ => {
                        out.push('%');
                        continue;
                    }
                }
                chars.next();
            }
            out
        })
        .collect()
}

/// Writes arguments back as one line that `split` reads as the same arguments, for the host
/// form and for `~/.ssh/config`.
pub fn join(argv: &[String]) -> String {
    fn plain(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '%' | '=' | '@' | ',' | '+')
    }
    argv.iter()
        .map(|arg| if !arg.is_empty() && arg.chars().all(plain) { arg.clone() } else { format!("'{}'", arg.replace('\'', r"'\''")) })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "proxy_command_tests.rs"]
mod tests;
