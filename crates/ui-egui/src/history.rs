//! Which typed lines become history (`history.record`): only lines typed after a shell prompt
//! (a line ending in `$`, `#`, `%`, `>`, `❯` or `»`, or starting with `➜`), decided when the
//! first key is typed, and only when the shell echoed the line back. Never kept: input nothing
//! echoes (a secret), typeahead before the next prompt, lines starting with a space, answers to
//! a password, passphrase, PIN, token or code question, input to a REPL or a continuation
//! prompt, anything in a full-screen program, lines changed by Tab completion, arrows or Ctrl+R,
//! multi-line pastes, and input while broadcast is on.

use crate::status::waiting_line;

/// What the pane shows when a line starts.
#[derive(Debug, Clone, Default)]
pub struct LineContext {
    /// The cursor's line up to the cursor, before the key: the prompt, and at Enter the prompt
    /// with the echoed line.
    pub prompt: String,
    /// The line above the cursor's: the start of a line that wrapped.
    pub above: String,
    /// A full-screen program runs (the alternate screen).
    pub alt_screen: bool,
    pub broadcast: bool,
}

/// Prompts of REPLs and continuation lines: what is typed there is not a shell command.
fn repl(prompt: &str) -> bool {
    let p = prompt.trim_end();
    p == ">"
        || p == "..."
        || p.ends_with("mysql>")
        || p.ends_with("=#")
        || p.ends_with("=>")
        || p.ends_with("sqlite>")
        || p.ends_with(">>>")
        || p.starts_with("irb(")
}

/// Whether `prompt` ends like a shell's prompt. Anything else (a program's question, a line of
/// output the typing raced) is no place for history.
fn shell_prompt(prompt: &str) -> bool {
    let p = prompt.trim_end();
    p.ends_with(['$', '#', '%', '>', '❯', '»']) || p.trim_start().starts_with('➜')
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Undecided,
    Shell,
    Skip,
}

#[derive(Debug, Clone, Default)]
enum Escape {
    #[default]
    None,
    Esc,
    Csi(Vec<u8>),
}

/// Most bytes one line keeps; a longer line is no history.
const MAX_LINE: usize = 2000;

/// The line being typed in one pane.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    line: Vec<u8>,
    kind: Kind,
    /// The shell changed the line (completion, history, cursor moves): what is typed here is not
    /// the whole line.
    edited: bool,
    escape: Escape,
    /// Inside a bracketed paste: what it pasted so far.
    paste: Option<Vec<u8>>,
}

impl Capture {
    fn decide(&mut self, ctx: &LineContext) {
        if self.kind == Kind::Undecided {
            let skip = ctx.alt_screen || ctx.broadcast || waiting_line(&ctx.prompt) || repl(&ctx.prompt) || !shell_prompt(&ctx.prompt);
            self.kind = if skip { Kind::Skip } else { Kind::Shell };
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn finish(&mut self, ctx: &LineContext) -> Option<String> {
        let text = String::from_utf8_lossy(&self.line).trim_end().to_string();
        // The shell showed the line before the cursor: what nothing echoes is a secret or was
        // typed ahead of the next prompt.
        let echoed = format!("{}{}", ctx.above, ctx.prompt).trim_end().ends_with(text.as_str());
        let keep = self.kind == Kind::Shell && echoed && !self.edited && !text.starts_with(' ') && !text.is_empty() && self.line.len() <= MAX_LINE;
        self.reset();
        keep.then_some(text)
    }

    /// The line typed so far at a shell prompt; `None` where nothing may be suggested.
    pub fn line(&self) -> Option<&str> {
        (self.kind == Kind::Shell && !self.edited && self.paste.is_none()).then(|| std::str::from_utf8(&self.line).ok()).flatten()
    }

    /// Takes the bytes typed into the pane; a line that ends here and counts as history.
    pub fn input(&mut self, bytes: &[u8], ctx: &LineContext) -> Option<String> {
        let mut kept = None;
        for &b in bytes {
            if let Some(paste) = self.paste.as_mut() {
                paste.push(b);
                if let Some(content) = paste.strip_suffix(b"\x1b[201~") {
                    if content.iter().any(|c| *c == b'\n' || *c == b'\r') {
                        self.kind = Kind::Skip;
                    } else {
                        let content = content.to_vec();
                        self.line.extend(content);
                    }
                    self.paste = None;
                }
                continue;
            }
            match std::mem::take(&mut self.escape) {
                Escape::Esc => {
                    if b == b'[' {
                        self.escape = Escape::Csi(Vec::new());
                    } else {
                        self.edited = true;
                    }
                    continue;
                }
                Escape::Csi(mut seq) => {
                    seq.push(b);
                    if (0x40..=0x7e).contains(&b) {
                        if seq == b"200~" {
                            self.paste = Some(Vec::new());
                        } else {
                            // Arrows, Home, End, Delete: the line moved under the cursor.
                            self.edited = true;
                        }
                    } else if seq.len() < 16 {
                        self.escape = Escape::Csi(seq);
                    }
                    continue;
                }
                Escape::None => {}
            }
            match b {
                0x1b => {
                    self.decide(ctx);
                    self.escape = Escape::Esc;
                }
                b'\r' | b'\n' => kept = self.finish(ctx).or(kept),
                0x7f | 0x08 => {
                    // One character back, whole UTF-8 sequences at a time.
                    while let Some(last) = self.line.pop() {
                        if last & 0xc0 != 0x80 {
                            break;
                        }
                    }
                }
                0x03 | 0x15 => self.reset(),
                b'\t' | 0x12 => {
                    self.decide(ctx);
                    self.edited = true;
                }
                c if c < 0x20 => {
                    self.decide(ctx);
                    self.edited = true;
                }
                c => {
                    self.decide(ctx);
                    if self.line.len() <= MAX_LINE {
                        self.line.push(c);
                    }
                }
            }
        }
        kept
    }
}
