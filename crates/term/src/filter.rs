//! Bounds the parser does not know: an OSC string (title, clipboard, link) is held back until it
//! is complete and then passed on whole; one longer than [`MAX_OSC_BYTES`] is dropped whole, so
//! a server that never ends one cannot make the terminal buffer without limit, and a cut-off
//! clipboard or link never takes effect. (Cancelling with `CAN` is not enough: the parser still
//! acts on what it has.)

use crate::limits::MAX_OSC_BYTES;

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

#[derive(Debug, Default)]
pub(crate) struct Prefilter {
    state: State,
    /// The OSC string being held back, starting with `ESC ]`.
    osc: Vec<u8>,
    /// The working directory the last OSC 7 named.
    pub(crate) cwd: Option<String>,
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
        if let Some(dir) = working_directory(&self.osc) {
            self.cwd = Some(dir);
        }
        out.extend_from_slice(&self.osc);
        if self.osc.capacity() > 1 << 16 {
            self.osc = Vec::new();
        } else {
            self.osc.clear();
        }
        State::Ground
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

/// The longest working directory OSC 7 may name, in characters.
const MAX_CWD_CHARS: usize = 1000;

/// The path of a complete `ESC ] 7 ; file://host/path` string: percent-decoded, one plain line,
/// at most [`MAX_CWD_CHARS`]; `None` for anything else.
fn working_directory(osc: &[u8]) -> Option<String> {
    let body = osc.strip_prefix(b"\x1b]7;")?;
    let body = body.strip_suffix(b"\x1b\\").or_else(|| body.strip_suffix(&[BEL]))?;
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
    fn an_overlong_osc_is_dropped_whole() {
        let body = vec![b'A'; MAX_OSC_BYTES + 10];
        let out = run(&[b"\x1b]0;", &body, b"\x1b\\ok"]);
        assert_eq!(out, b"ok");
    }
}
