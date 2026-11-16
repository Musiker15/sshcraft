//! tmux control mode (`tmux -CC`), as the tmux manual describes it: tmux sends one notification
//! per line (`%output %1 text`, `%window-add @2`, ...) and answers each command between
//! `%begin` and `%end` (or `%error`). SSHCraft shows each tmux window as a tab and each pane as
//! a terminal. Everything here is the server's and so hostile: lines, blocks and ids are bounded,
//! and what does not parse is skipped.

/// The longest line kept; a longer one is dropped up to its end.
pub const MAX_LINE: usize = 1 << 20;
/// Lines kept of one command's answer.
const MAX_BLOCK_LINES: usize = 10_000;
/// Panes one layout may name.
const MAX_LAYOUT_PANES: usize = 256;
/// Bytes per `send-keys` command.
pub const SEND_CHUNK: usize = 256;

/// What tmux told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Output of pane `%pane`.
    Output {
        pane: u32,
        data: Vec<u8>,
    },
    WindowAdd(u32),
    WindowClose(u32),
    WindowRenamed {
        window: u32,
        name: String,
    },
    /// The panes of window `@window`, in layout order.
    LayoutChange {
        window: u32,
        panes: Vec<u32>,
    },
    /// The active pane of a window changed.
    PaneFocus {
        window: u32,
        pane: u32,
    },
    SessionChanged {
        name: String,
    },
    /// The answer to a command: its lines, and whether it failed.
    Reply {
        ok: bool,
        lines: Vec<String>,
    },
    /// tmux left control mode (the session ended, or it was detached).
    Exit(Option<String>),
}

/// Reads tmux's control mode output.
#[derive(Debug, Default)]
pub struct Parser {
    line: Vec<u8>,
    /// The line being read is longer than [`MAX_LINE`]: dropped up to its end.
    skipping: bool,
    /// Inside a `%begin` block: its lines so far.
    block: Option<Vec<String>>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes of an unfinished line held now; never more than [`MAX_LINE`].
    pub fn buffered(&self) -> usize {
        self.line.len()
    }

    /// Takes what came from the server and returns the events of every complete line.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                let line = std::mem::take(&mut self.line);
                if !std::mem::take(&mut self.skipping) {
                    self.line_done(&line, &mut out);
                }
            } else if !self.skipping {
                if self.line.len() >= MAX_LINE {
                    self.line.clear();
                    self.skipping = true;
                } else {
                    self.line.push(b);
                }
            }
        }
        out
    }

    fn line_done(&mut self, raw: &[u8], out: &mut Vec<Event>) {
        // `-CC` wraps the whole stream in a DCS sequence (`ESC P 1000 p`) and a trailing `\r`
        // may come with a terminal in between.
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        const DCS: &[u8] = b"\x1bP1000p";
        let raw = match raw.windows(DCS.len()).position(|w| w == DCS) {
            Some(at) => raw.get(at + DCS.len()..).unwrap_or_default(),
            None => raw,
        };
        if let Some(lines) = &mut self.block {
            let text = String::from_utf8_lossy(raw);
            if text.starts_with("%end") || text.starts_with("%error") {
                let ok = text.starts_with("%end");
                let lines = self.block.take().unwrap_or_default();
                out.push(Event::Reply { ok, lines });
            } else if lines.len() < MAX_BLOCK_LINES {
                lines.push(text.into_owned());
            }
            return;
        }
        let Some(rest) = raw.strip_prefix(b"%") else { return };
        let (word, args) = match rest.iter().position(|&b| b == b' ') {
            Some(at) => (rest.get(..at).unwrap_or_default(), rest.get(at + 1..).unwrap_or_default()),
            None => (rest, &[][..]),
        };
        let text = || String::from_utf8_lossy(args).into_owned();
        match word {
            b"begin" => self.block = Some(Vec::new()),
            b"output" => {
                if let Some((pane, data)) = split_id(args, b'%') {
                    out.push(Event::Output { pane, data: unescape(data) });
                }
            }
            b"window-add" => out.extend(id(args, b'@').map(Event::WindowAdd)),
            b"window-close" | b"unlinked-window-close" => out.extend(id(args, b'@').map(Event::WindowClose)),
            b"window-renamed" => {
                if let Some((window, name)) = split_id(args, b'@') {
                    out.push(Event::WindowRenamed { window, name: clean(&String::from_utf8_lossy(name)) });
                }
            }
            b"layout-change" => {
                let t = text();
                let mut parts = t.split(' ');
                if let (Some(window), Some(layout)) = (parts.next().and_then(|w| id(w.as_bytes(), b'@')), parts.next()) {
                    out.push(Event::LayoutChange { window, panes: layout_panes(layout) });
                }
            }
            b"window-pane-changed" => {
                let t = text();
                let mut parts = t.split(' ');
                if let (Some(window), Some(pane)) =
                    (parts.next().and_then(|w| id(w.as_bytes(), b'@')), parts.next().and_then(|p| id(p.as_bytes(), b'%')))
                {
                    out.push(Event::PaneFocus { window, pane });
                }
            }
            b"session-changed" => {
                if let Some((_, name)) = split_id(args, b'$') {
                    out.push(Event::SessionChanged { name: clean(&String::from_utf8_lossy(name)) });
                }
            }
            b"exit" => out.push(Event::Exit((!args.is_empty()).then(|| clean(&text())))),
            _ => {}
        }
    }
}

/// `@12` (with `sigil` `@`) as 12.
fn id(text: &[u8], sigil: u8) -> Option<u32> {
    let digits = text.strip_prefix(&[sigil])?;
    if digits.is_empty() || digits.len() > 9 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// `%3 rest` as (3, rest).
fn split_id(args: &[u8], sigil: u8) -> Option<(u32, &[u8])> {
    let at = args.iter().position(|&b| b == b' ').unwrap_or(args.len());
    let rest = args.get(at + 1..).unwrap_or_default();
    Some((id(args.get(..at)?, sigil)?, rest))
}

/// A name for the tab: no control characters, at most 128 characters.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(128).collect()
}

/// `%output` escapes bytes below 32 and the backslash as `\ooo`; anything else is literal.
pub fn unescape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while let Some(&b) = data.get(i) {
        if b == b'\\'
            && let Some(digits) = data.get(i + 1..i + 4)
            && digits.iter().all(|d| (b'0'..=b'7').contains(d))
        {
            let value = digits.iter().fold(0u16, |acc, d| acc * 8 + u16::from(d - b'0'));
            if let Ok(byte) = u8::try_from(value) {
                out.push(byte);
                i += 4;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

/// The pane ids of a tmux layout (`b25f,80x24,0,0,1` or `e2b0,80x24,0,0{40x24,0,0,1,39x24,41,0,2}`):
/// each leaf cell is `WxH,X,Y,ID`.
pub fn layout_panes(layout: &str) -> Vec<u32> {
    // The checksum and the comma after it come first.
    let body = layout.split_once(',').map_or("", |(_, b)| b);
    let mut panes = Vec::new();
    for cell in body.split(['{', '}', '[', ']']) {
        // A cell list is `WxH,X,Y,ID,WxH,X,Y,ID...` or ends with `WxH,X,Y` before a bracket.
        let fields: Vec<&str> = cell.split(',').filter(|f| !f.is_empty()).collect();
        let mut i = 0;
        while let Some(size) = fields.get(i) {
            if !size.contains('x') {
                i += 1;
                continue;
            }
            match fields.get(i + 3) {
                Some(id) if !id.contains('x') => {
                    if let Ok(id) = id.parse::<u32>()
                        && panes.len() < MAX_LAYOUT_PANES
                        && !panes.contains(&id)
                    {
                        panes.push(id);
                    }
                    i += 4;
                }
                _ => i += 3,
            }
        }
    }
    panes
}

/// The command that types `bytes` into pane `%pane`, as hex keys; one command per
/// [`SEND_CHUNK`] bytes.
pub fn send_keys(pane: u32, bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(SEND_CHUNK)
        .map(|chunk| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            format!("send-keys -t %{pane} -H {}\n", hex.join(" "))
        })
        .collect()
}

/// The command that tells tmux the size of the window it draws into.
pub fn resize(cols: u16, rows: u16) -> String {
    format!("refresh-client -C {}x{}\n", cols.max(1), rows.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_are_read() {
        let mut p = Parser::new();
        let events = p.feed(
            b"\x1bP1000p%begin 1 2 0\n%end 1 2 0\n%window-add @1\n%output %0 hello\\015\\012\n%window-renamed @1 vim main.rs\n%layout-change @1 e2b0,80x24,0,0{40x24,0,0,1,39x24,41,0,2} e2b0,80x24,0,0{40x24,0,0,1,39x24,41,0,2} *\n%window-pane-changed @1 %2\n%session-changed $0 work\n%window-close @1\n%exit detached\n",
        );
        assert_eq!(
            events,
            vec![
                Event::Reply { ok: true, lines: vec![] },
                Event::WindowAdd(1),
                Event::Output { pane: 0, data: b"hello\r\n".to_vec() },
                Event::WindowRenamed { window: 1, name: "vim main.rs".into() },
                Event::LayoutChange { window: 1, panes: vec![1, 2] },
                Event::PaneFocus { window: 1, pane: 2 },
                Event::SessionChanged { name: "work".into() },
                Event::WindowClose(1),
                Event::Exit(Some("detached".into())),
            ]
        );
    }

    #[test]
    fn a_reply_keeps_its_lines_and_failures_say_so() {
        let mut p = Parser::new();
        assert_eq!(p.feed(b"%begin 1 5 1\n@1 %0 main\n%error 1 5 1\n"), vec![Event::Reply { ok: false, lines: vec!["@1 %0 main".into()] }]);
    }

    #[test]
    fn lines_may_arrive_in_pieces() {
        let mut p = Parser::new();
        assert!(p.feed(b"%output %3 ab").is_empty());
        assert_eq!(p.feed(b"c\n"), vec![Event::Output { pane: 3, data: b"abc".to_vec() }]);
    }

    #[test]
    fn layouts_name_their_panes() {
        assert_eq!(layout_panes("b25f,80x24,0,0,5"), vec![5]);
        assert_eq!(layout_panes("a1b2,80x24,0,0[80x12,0,0,1,80x11,0,13{40x11,0,13,2,39x11,41,13,3}]"), vec![1, 2, 3]);
        assert!(layout_panes("garbage").is_empty());
    }

    #[test]
    fn hostile_input_is_bounded() {
        let mut p = Parser::new();
        // A line without end, then a normal one.
        let mut long = vec![b'x'; MAX_LINE + 10];
        long.extend_from_slice(b"\n%window-add @7\n");
        assert_eq!(p.feed(&long), vec![Event::WindowAdd(7)]);
        assert!(p.feed(b"%output %99999999999 x\n").is_empty(), "an id past u32");
        assert!(p.feed(b"%window-add @\n%window-add 7\n%layout-change\n").is_empty());
        assert_eq!(unescape(b"\\999\\0\\\\134"), b"\\999\\0\\\\".to_vec());
        assert_eq!(unescape(b"a\\"), b"a\\".to_vec());
        let many = format!("x,1x1,0,0{}", ",1x1,0,0,1".repeat(5000));
        assert!(layout_panes(&many).len() <= MAX_LAYOUT_PANES);
    }

    #[test]
    fn a_line_without_end_is_dropped_not_kept() {
        let mut p = Parser::new();
        for _ in 0..3 {
            assert!(p.feed(&vec![b'x'; MAX_LINE]).is_empty());
            assert!(p.buffered() <= MAX_LINE);
        }
        assert_eq!(
            p.feed(
                b"
%window-add @1
"
            ),
            vec![Event::WindowAdd(1)]
        );
    }

    #[test]
    fn commands_are_built_from_numbers_and_hex_only() {
        assert_eq!(send_keys(2, b"ls\r"), vec!["send-keys -t %2 -H 6c 73 0d\n".to_string()]);
        assert_eq!(send_keys(1, &[b'a'; 300]).len(), 2);
        assert_eq!(resize(0, 24), "refresh-client -C 1x24\n");
    }
}
