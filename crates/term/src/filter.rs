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
                self.osc = Vec::new();
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
            State::DiscardEscape => match byte {
                b'\\' => State::Ground,
                _ => Self::dropping(byte),
            },
        }
    }

    /// Passes the held-back OSC string on and goes back to ground, keeping `ESC` pending when the
    /// string was ended by one.
    fn release(&mut self, out: &mut Vec<u8>) -> State {
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
