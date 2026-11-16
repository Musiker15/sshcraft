//! Bounds the parser does not know: an OSC string (title, clipboard, link) is held back until it
//! is complete and then passed on whole; one longer than [`MAX_OSC_BYTES`] is dropped whole, so
//! a server that never ends one cannot make the terminal buffer without limit, and a cut-off
//! clipboard or link never takes effect. (Cancelling with `CAN` is not enough: the parser still
//! acts on what it has.)

use crate::limits::MAX_OSC_BYTES;
use crate::progstatus::{self, Report};

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    /// `ESC` seen outside an OSC string.
    Escape,
    /// Inside an OSC string; its bytes so far are held in [`Prefilter::osc`].
    Osc,
    /// `ESC` seen inside an OSC string.
    OscEscape,
    /// The OSC string passed the limit: everything is dropped until it ends.
    Discard,
    /// `ESC` seen while dropping.
    DiscardEscape,
}

/// What OSC 133 (shell integration) says about the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellMark {
    /// `A`: a prompt starts.
    PromptStart,
    /// `B`: the prompt ends, the user types the command.
    CommandStart,
    /// `C`: the command runs, its output starts.
    OutputStart,
    /// `D`: the command ended, with its exit code when one was given and is a number.
    CommandEnd(Option<i32>),
}

/// Something the terminal applies once the parser reached the point where it came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FilterEvent {
    Shell(ShellMark),
    /// A program status report (OSC 7501).
    Status(Report),
    /// A full reset (`ESC c`).
    Reset,
}

/// Events kept for one feed; more are dropped (each takes at least two bytes of input).
const MAX_EVENTS: usize = 1 << 16;
/// Notifications kept for one feed; the interface limits how many it shows anyway.
const MAX_NOTIFICATIONS: usize = 64;
/// Answers kept for one feed.
const MAX_REPLIES: usize = 16;
/// The longest notification title and body, in characters.
pub(crate) const MAX_NOTIFY_TITLE: usize = 128;
pub(crate) const MAX_NOTIFY_BODY: usize = 1024;

#[derive(Debug, Default)]
pub(crate) struct Prefilter {
    state: State,
    /// The OSC string being held back, starting with `ESC ]`.
    osc: Vec<u8>,
    /// The working directory the last OSC 7 named.
    pub(crate) cwd: Option<String>,
    /// Events of this feed, each with the length of the filtered output when it came: the parser
    /// has to reach that point before it applies (a shell mark to the cursor line, for example).
    pub(crate) events: Vec<(usize, FilterEvent)>,
    /// Desktop notifications (OSC 9, OSC 777): title and body.
    pub(crate) notifications: Vec<(Option<String>, String)>,
    /// Answers to send back (the OSC 7501 capability query).
    pub(crate) replies: Vec<Vec<u8>>,
}

impl Prefilter {
    /// Appends what the parser may see of `input` to `out`.
    pub(crate) fn filter(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &byte in input {
            self.state = self.step(byte, out);
        }
    }

    fn step(&mut self, byte: u8, out: &mut Vec<u8>) -> State {
        match self.state {
            State::Ground => {
                if byte == ESC {
                    State::Escape
                } else {
                    out.push(byte);
                    State::Ground
                }
            }
            State::Escape => match byte {
                b']' => {
                    self.osc.clear();
                    self.osc.extend_from_slice(&[ESC, b']']);
                    State::Osc
                }
                ESC => {
                    out.push(ESC);
                    State::Escape
                }
                _ => {
                    out.extend_from_slice(&[ESC, byte]);
                    if byte == b'c' {
                        self.event(out.len(), FilterEvent::Reset);
                    }
                    State::Ground
                }
            },
            State::Osc | State::OscEscape if self.osc.len() >= MAX_OSC_BYTES => {
                let after_escape = self.state == State::OscEscape;
                self.osc = Vec::new();
                if after_escape {
                    // The `ESC` already held is the start of the string's end or of a new sequence.
                    self.state = State::DiscardEscape;
                    return self.step(byte, out);
                }
                Self::dropping(byte)
            }
            State::Osc => {
                self.osc.push(byte);
                match byte {
                    BEL | CAN | SUB => self.release(out),
                    ESC => State::OscEscape,
                    _ => State::Osc,
                }
            }
            State::OscEscape => {
                if byte == b'\\' {
                    self.osc.push(byte);
                    return self.release(out);
                }
                // `ESC` ended the string and starts a new sequence: hand both over.
                let _ = self.osc.pop();
                self.release(out);
                self.state = State::Escape;
                self.step(byte, out)
            }
            State::Discard => Self::dropping(byte),
            State::DiscardEscape => {
                if byte == b'\\' {
                    return State::Ground;
                }
                // Like the parser, any other byte after `ESC` ends the string and starts a new
                // sequence, which is passed on.
                self.state = State::Escape;
                self.step(byte, out)
            }
        }
    }

    /// Passes the held-back OSC string on and goes back to ground, keeping `ESC` pending when the
    /// string was ended by one.
    fn release(&mut self, out: &mut Vec<u8>) -> State {
        // The parser knows none of OSC 133, 7501, 9 and 777, so they are not passed on.
        if let Some(mark) = shell_mark(&self.osc) {
            self.event(out.len(), FilterEvent::Shell(mark));
        } else if let Some(body) = osc_body(&self.osc, b"\x1b]7501;") {
            match progstatus::parse(body).filter(|_| self.osc.len() <= progstatus::MAX_REPORT_BYTES) {
                Some(Report::Query) => {
                    if self.replies.len() < MAX_REPLIES {
                        let end: &[u8] = if self.osc.ends_with(&[BEL]) { &[BEL] } else { b"\x1b\\" };
                        self.replies.push([b"\x1b]7501;?", end].concat());
                    }
                }
                Some(report) => self.event(out.len(), FilterEvent::Status(report)),
                None => {}
            }
        } else if let Some(notification) = notification(&self.osc) {
            if self.notifications.len() < MAX_NOTIFICATIONS {
                self.notifications.push(notification);
            }
        } else {
            if let Some(dir) = working_directory(&self.osc) {
                self.cwd = Some(dir);
            }
            out.extend_from_slice(&self.osc);
        }
        if self.osc.capacity() > 1 << 16 {
            self.osc = Vec::new();
        } else {
            self.osc.clear();
        }
        State::Ground
    }

    fn event(&mut self, offset: usize, event: FilterEvent) {
        if self.events.len() < MAX_EVENTS {
            self.events.push((offset, event));
        }
    }

    /// The state after dropping `byte` of an overlong OSC string.
    fn dropping(byte: u8) -> State {
        match byte {
            BEL | CAN | SUB => State::Ground,
            ESC => State::DiscardEscape,
            _ => State::Discard,
        }
    }
}

/// The body of a complete OSC string after `prefix`, without its terminator.
fn osc_body<'a>(osc: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let body = osc.strip_prefix(prefix)?;
    body.strip_suffix(b"\x1b\\").or_else(|| body.strip_suffix(&[BEL]))
}

/// The mark of a complete `ESC ] 133 ; <kind> [; ...]` string; `None` for anything else.
fn shell_mark(osc: &[u8]) -> Option<ShellMark> {
    let body = osc_body(osc, b"\x1b]133;")?;
    let mut parts = body.split(|b| *b == b';');
    match parts.next()? {
        b"A" => Some(ShellMark::PromptStart),
        b"B" => Some(ShellMark::CommandStart),
        b"C" => Some(ShellMark::OutputStart),
        b"D" => {
            let code = parts.next().and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse::<i32>().ok());
            Some(ShellMark::CommandEnd(code))
        }
        _ => None,
    }
}

/// A desktop notification from `ESC ] 9 ; body` or `ESC ] 777 ; notify ; title ; body`, as plain
/// bounded text. `ESC ] 9 ; 4 ; ...` and other numbered forms are progress reports of ConEmu, not
/// notifications.
fn notification(osc: &[u8]) -> Option<(Option<String>, String)> {
    if let Some(body) = osc_body(osc, b"\x1b]9;") {
        let text = String::from_utf8_lossy(body);
        let numbered = text.split_once(';').is_some_and(|(first, _)| !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()));
        if numbered {
            return None;
        }
        return Some((None, crate::clean_text(&text, MAX_NOTIFY_BODY)));
    }
    let body = osc_body(osc, b"\x1b]777;notify;")?;
    let text = String::from_utf8_lossy(body);
    let (title, body) = text.split_once(';').unwrap_or((&text, ""));
    Some((Some(crate::clean_text(title, MAX_NOTIFY_TITLE)), crate::clean_text(body, MAX_NOTIFY_BODY)))
}

/// The longest working directory OSC 7 may name, in characters.
const MAX_CWD_CHARS: usize = 1000;

/// The path of a complete `ESC ] 7 ; file://host/path` string: percent-decoded, one plain line,
/// at most [`MAX_CWD_CHARS`]; `None` for anything else.
fn working_directory(osc: &[u8]) -> Option<String> {
    let body = osc_body(osc, b"\x1b]7;")?;
    let rest = body.strip_prefix(b"file://")?;
    let path = rest.get(rest.iter().position(|b| *b == b'/')?..)?;
    let mut bytes = Vec::with_capacity(path.len());
    let mut i = 0;
    while let Some(&b) = path.get(i) {
        if b == b'%' {
            let hex = std::str::from_utf8(path.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(b);
            i += 1;
        }
    }
    let dir = String::from_utf8(bytes).ok()?;
    (dir.chars().count() <= MAX_CWD_CHARS && !dir.chars().any(char::is_control)).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> Vec<u8> {
        let mut filter = Prefilter::default();
        let mut out = Vec::new();
        for chunk in chunks {
            filter.filter(chunk, &mut out);
        }
        out
    }

    #[test]
    fn ordinary_output_passes_unchanged() {
        let input: &[u8] = b"a\x1b[31mb\x1b]0;title\x07c\x1b]8;;x\x1b\\d\x1b\x1b[0m\x1b]0;t\x1b[1me";
        assert_eq!(run(&[input]), input);
    }

    #[test]
    fn an_overlong_osc_ended_by_a_new_sequence_lets_output_through() {
        let body = vec![b'A'; MAX_OSC_BYTES + 10];
        assert_eq!(run(&[b"\x1b]52;c;", &body, b"\x1b[31mok\r\nmore"]), b"\x1b[31mok\r\nmore");
    }

    #[test]
    fn an_osc_whose_terminator_lands_on_the_limit_ends_cleanly() {
        // `ESC ]` plus the body plus `ESC` reach the limit exactly; `\` completes the terminator.
        let body = vec![b'A'; MAX_OSC_BYTES - 3];
        assert_eq!(run(&[b"\x1b]", &body, b"\x1b\\visible"]), b"visible");
    }

    #[test]
    fn a_split_osc_passes_whole() {
        assert_eq!(run(&[b"x\x1b", b"]0;ti", b"tle\x1b", b"\\y"]), b"x\x1b]0;title\x1b\\y");
    }

    #[test]
    fn shell_marks_are_held_back_with_their_offset() {
        let mut filter = Prefilter::default();
        let mut out = Vec::new();
        filter.filter(b"ab\x1b]133;A\x07cd\x1b]133;D;7\x1b\\e\x1b]133;D;x\x07", &mut out);
        assert_eq!(out, b"abcde");
        assert_eq!(
            filter.events,
            vec![
                (2, FilterEvent::Shell(ShellMark::PromptStart)),
                (4, FilterEvent::Shell(ShellMark::CommandEnd(Some(7)))),
                (5, FilterEvent::Shell(ShellMark::CommandEnd(None)))
            ]
        );
    }

    #[test]
    fn an_overlong_osc_is_dropped_whole() {
        let body = vec![b'A'; MAX_OSC_BYTES + 10];
        let out = run(&[b"\x1b]0;", &body, b"\x1b\\ok"]);
        assert_eq!(out, b"ok");
    }
}
