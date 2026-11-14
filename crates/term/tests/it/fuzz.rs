use sshcraft_term::TermSize;

use crate::term;

/// A small, fast generator with a fixed sequence per seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const PIECES: &[&[u8]] = &[
    b"\x1b",
    b"[",
    b"]",
    b";",
    b"?",
    b"0",
    b"1",
    b"9",
    b"65535",
    b"\x07",
    b"\x1b\\",
    b"m",
    b"H",
    b"J",
    b"K",
    b"@",
    b"\n",
    b"\r",
    b"\x08",
    b"\t",
    b"\xe6\x97",
    b"\xa5",
    b"\xf0\x9f",
    b"\xcc\x81",
    b"8;;",
    b"52;c;",
    b"P",
    b"q",
    b"t",
    b"h",
    b"l",
    b"r",
];

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0x5eed_cafe);
    let mut t = term(80, 24);
    for _ in 0..2000 {
        let len = 1 + rng.below(4096) as usize;
        let mut chunk = Vec::with_capacity(len);
        while chunk.len() < len {
            if rng.below(3) == 0 {
                chunk.push(rng.next() as u8);
            } else {
                chunk.extend_from_slice(PIECES[rng.below(PIECES.len() as u64) as usize]);
            }
        }
        t.feed(&chunk);
        if rng.below(20) == 0 {
            t.resize(TermSize::clamped(rng.below(300) as u16, rng.below(120) as u16));
        }
        let s = t.screen();
        assert_eq!(s.cells.len(), usize::from(s.cols) * usize::from(s.rows));
        let _ = t.take_notices();
    }
}
