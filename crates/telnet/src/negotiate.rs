//! Telnet option negotiation (RFC 854 and 855) for a client: what the server sends is split into
//! terminal output and answers. SSHCraft accepts the options a terminal needs (echo and suppress
//! go-ahead from the server; terminal type (RFC 1091), window size (RFC 1073) and binary from
//! itself) and refuses every other one. Answers are bounded, so a server that keeps negotiating
//! cannot make the client loop.

/// Interpret as command.
pub const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

pub const BINARY: u8 = 0;
pub const ECHO: u8 = 1;
pub const SGA: u8 = 3;
pub const TTYPE: u8 = 24;
pub const NAWS: u8 = 31;

/// `IS` and `SEND` in a terminal type subnegotiation.
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// Answers sent at most per connection; past that, negotiation is ignored.
pub const MAX_REPLIES: usize = 1024;
/// The longest subnegotiation kept.
const MAX_SB: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    /// After WILL, WONT, DO or DONT: the option follows.
    Verb(u8),
    Sb,
    SbIac,
}

/// One connection's negotiation.
#[derive(Debug)]
pub struct Negotiator {
    state: State,
    sb: Vec<u8>,
    /// Options the server does (it said WILL and we said DO).
    remote: [bool; 256],
    /// Options we do (the server said DO and we said WILL).
    local: [bool; 256],
    replies: usize,
    term: String,
    size: (u16, u16),
}

/// What one read gave.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Read {
    /// Bytes for the terminal.
    pub data: Vec<u8>,
    /// Bytes to send back to the server.
    pub reply: Vec<u8>,
}

impl Negotiator {
    /// `term` is the terminal type told to the server (`xterm-256color`), `size` its columns and
    /// rows.
    pub fn new(term: &str, size: (u16, u16)) -> Self {
        let term = term.chars().filter(|c| c.is_ascii_graphic()).take(40).collect::<String>().to_ascii_uppercase();
        Self { state: State::Data, sb: Vec::new(), remote: [false; 256], local: [false; 256], replies: 0, term, size }
    }

    /// Splits `bytes` from the server into output and answers.
    pub fn read(&mut self, bytes: &[u8]) -> Read {
        let mut out = Read::default();
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, _) => {
                    out.data.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    out.data.push(IAC);
                    State::Data
                }
                (State::Iac, WILL | WONT | DO | DONT) => State::Verb(b),
                (State::Iac, SB) => {
                    self.sb.clear();
                    State::Sb
                }
                // NOP, go-ahead, data mark and the rest carry nothing for a terminal.
                (State::Iac, _) => State::Data,
                (State::Verb(verb), option) => {
                    self.verb(verb, option, &mut out.reply);
                    State::Data
                }
                (State::Sb, IAC) => State::SbIac,
                (State::Sb, _) => {
                    if self.sb.len() < MAX_SB {
                        self.sb.push(b);
                    }
                    State::Sb
                }
                (State::SbIac, SE) => {
                    self.subnegotiation(&mut out.reply);
                    State::Data
                }
                (State::SbIac, IAC) => {
                    if self.sb.len() < MAX_SB {
                        self.sb.push(IAC);
                    }
                    State::Sb
                }
                // A broken subnegotiation: dropped.
                (State::SbIac, _) => State::Data,
            };
        }
        out
    }

    fn answer(&mut self, reply: &mut Vec<u8>, bytes: &[u8]) -> bool {
        if self.replies >= MAX_REPLIES {
            return false;
        }
        self.replies += 1;
        reply.extend_from_slice(bytes);
        true
    }

    fn verb(&mut self, verb: u8, option: u8, reply: &mut Vec<u8>) {
        let i = usize::from(option);
        match verb {
            WILL => {
                if matches!(option, ECHO | SGA | BINARY) {
                    // Only a change is answered, so two sides cannot answer each other forever.
                    if !self.remote[i] && self.answer(reply, &[IAC, DO, option]) {
                        self.remote[i] = true;
                    }
                } else {
                    // Refused; the answer budget bounds a server that keeps offering.
                    let _ = self.answer(reply, &[IAC, DONT, option]);
                }
            }
            // Only a change is answered.
            WONT if self.remote[i] && self.answer(reply, &[IAC, DONT, option]) => self.remote[i] = false,
            DO => {
                let wanted = matches!(option, TTYPE | NAWS | BINARY | SGA);
                if wanted {
                    if !self.local[i] && self.answer(reply, &[IAC, WILL, option]) {
                        self.local[i] = true;
                    }
                    if option == NAWS {
                        let size = self.naws();
                        let _ = self.answer(reply, &size);
                    }
                } else {
                    let _ = self.answer(reply, &[IAC, WONT, option]);
                }
            }
            // Only a change is answered.
            DONT if self.local[i] && self.answer(reply, &[IAC, WONT, option]) => self.local[i] = false,
            _ => {}
        }
    }

    fn subnegotiation(&mut self, reply: &mut Vec<u8>) {
        if self.sb.first() == Some(&TTYPE) && self.sb.get(1) == Some(&TTYPE_SEND) && self.local[usize::from(TTYPE)] {
            let mut answer = vec![IAC, SB, TTYPE, TTYPE_IS];
            answer.extend_from_slice(self.term.as_bytes());
            answer.extend_from_slice(&[IAC, SE]);
            let _ = self.answer(reply, &answer);
        }
    }

    /// The window size subnegotiation, 255 doubled.
    fn naws(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, NAWS];
        for half in [self.size.0, self.size.1] {
            for byte in half.to_be_bytes() {
                out.push(byte);
                if byte == IAC {
                    out.push(IAC);
                }
            }
        }
        out.extend_from_slice(&[IAC, SE]);
        out
    }

    /// The terminal changed its size: what to tell the server, if it asked for sizes.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.size = (cols, rows);
        self.local[usize::from(NAWS)].then(|| self.naws())
    }

    /// Typed bytes as they go to the server: 255 doubled, and a carriage return followed by NUL
    /// outside binary mode (RFC 854).
    pub fn write(&self, bytes: &[u8]) -> Vec<u8> {
        let binary = self.local[usize::from(BINARY)];
        let mut out = Vec::with_capacity(bytes.len() + 4);
        let mut iter = bytes.iter().peekable();
        while let Some(&b) = iter.next() {
            out.push(b);
            if b == IAC {
                out.push(IAC);
            } else if b == b'\r' && !binary && iter.peek() != Some(&&b'\n') {
                out.push(0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n() -> Negotiator {
        Negotiator::new("xterm-256color", (80, 24))
    }

    #[test]
    fn data_passes_and_doubled_iac_is_one_byte() {
        let r = n().read(b"login: \xff\xff!");
        assert_eq!(r.data, b"login: \xff!".to_vec());
        assert!(r.reply.is_empty());
    }

    #[test]
    fn wanted_options_are_taken_and_others_refused() {
        let mut neg = n();
        let r = neg.read(&[IAC, WILL, ECHO, IAC, WILL, 99, IAC, DO, NAWS, IAC, DO, 42]);
        let mut want = vec![IAC, DO, ECHO, IAC, DONT, 99, IAC, WILL, NAWS, IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE];
        want.extend_from_slice(&[IAC, WONT, 42]);
        assert_eq!(r.reply, want);
        // Said again: no new answer for what is already agreed.
        assert_eq!(neg.read(&[IAC, WILL, ECHO]).reply, Vec::<u8>::new());
    }

    #[test]
    fn the_terminal_type_is_told_when_asked() {
        let mut neg = n();
        let _ = neg.read(&[IAC, DO, TTYPE]);
        let r = neg.read(&[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE]);
        let mut want = vec![IAC, SB, TTYPE, TTYPE_IS];
        want.extend_from_slice(b"XTERM-256COLOR");
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(r.reply, want);
    }

    #[test]
    fn a_size_with_255_doubles_it() {
        let mut neg = n();
        let _ = neg.read(&[IAC, DO, NAWS]);
        assert_eq!(neg.resize(255, 10), Some(vec![IAC, SB, NAWS, 0, IAC, IAC, 0, 10, IAC, SE]));
    }

    #[test]
    fn typing_doubles_iac_and_pads_a_lone_cr() {
        let neg = n();
        assert_eq!(neg.write(b"ls\r"), b"ls\r\0".to_vec());
        assert_eq!(neg.write(b"a\r\nb\xff"), b"a\r\nb\xff\xff".to_vec());
    }

    #[test]
    fn endless_negotiation_is_bounded() {
        let mut neg = n();
        let mut flood = Vec::new();
        for _ in 0..10_000 {
            flood.extend_from_slice(&[IAC, DO, 42, IAC, WILL, 77]);
        }
        let r = neg.read(&flood);
        assert!(r.reply.len() <= MAX_REPLIES * 3);
        assert!(r.data.is_empty());
    }

    #[test]
    fn a_split_or_broken_sequence_never_loses_data() {
        let mut neg = n();
        assert_eq!(neg.read(&[b'a', IAC]).data, b"a".to_vec());
        assert_eq!(neg.read(&[IAC, b'b']).data, vec![IAC, b'b']);
        // A subnegotiation that never ends keeps at most its bound.
        let mut long = vec![IAC, SB, TTYPE];
        long.extend(std::iter::repeat_n(7u8, 100_000));
        let _ = neg.read(&long);
        assert!(neg.sb.len() <= MAX_SB);
    }
}
