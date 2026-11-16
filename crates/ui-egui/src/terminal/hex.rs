//! The hex view: what a session received, byte by byte, while the view is open. Useful for
//! debugging a program's escape sequences or a device's odd characters. Nothing is kept while
//! the view is closed, and at most [`MAX_BYTES`] while it is open.

use std::collections::VecDeque;

/// Bytes kept while the view is open; older ones drop out.
pub const MAX_BYTES: usize = 64 * 1024;
/// Bytes per line.
const PER_LINE: usize = 16;

/// The last [`MAX_BYTES`] bytes a session received, and how many came before them.
#[derive(Debug, Default, Clone)]
pub struct HexBuffer {
    bytes: VecDeque<u8>,
    /// Bytes received since the view opened before the first one still kept (the offset of the
    /// first kept byte).
    dropped: usize,
}

impl HexBuffer {
    pub fn push(&mut self, chunk: &[u8]) {
        // Only the tail of a large chunk can stay; what is cut counts as dropped.
        let cut = chunk.len().saturating_sub(MAX_BYTES);
        let data = chunk.get(cut..).unwrap_or_default();
        let over = (self.bytes.len() + data.len()).saturating_sub(MAX_BYTES);
        self.bytes.drain(..over.min(self.bytes.len()));
        self.dropped = self.dropped.saturating_add(cut).saturating_add(over);
        self.bytes.extend(data);
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Where the first whole line starts in the kept bytes (lines start at multiples of 16).
    fn skip(&self) -> usize {
        ((PER_LINE - self.dropped % PER_LINE) % PER_LINE).min(self.bytes.len())
    }

    /// How many lines the kept bytes make.
    pub fn line_count(&self) -> usize {
        (self.bytes.len() - self.skip()).div_ceil(PER_LINE)
    }

    /// Lines `range` of the dump only (what is in view), so a flood does not format 64 KiB
    /// every frame.
    pub fn rows(&self, range: std::ops::Range<usize>) -> Vec<String> {
        let skip = self.skip();
        let start = range.start.saturating_mul(PER_LINE).saturating_add(skip).min(self.bytes.len());
        let end = range.end.saturating_mul(PER_LINE).saturating_add(skip).min(self.bytes.len());
        let bytes: Vec<u8> = self.bytes.range(start..end).copied().collect();
        hex_dump(&bytes, self.dropped.saturating_add(start))
    }

    /// The kept bytes as hex dump lines, offsets counted from when the view opened.
    pub fn lines(&self) -> Vec<String> {
        let (a, b) = self.bytes.as_slices();
        let mut all = Vec::with_capacity(self.bytes.len());
        all.extend_from_slice(a);
        all.extend_from_slice(b);
        // Lines start at multiples of 16 of the session's offset.
        let skip = (PER_LINE - self.dropped % PER_LINE) % PER_LINE;
        let start = self.dropped + skip;
        hex_dump(all.get(skip.min(all.len())..).unwrap_or_default(), start)
    }
}

/// `bytes` as lines like `00000010  1b 5b 33 31 6d 68 69 0d  0a 00 ...  |.[31mhi...|`; the
/// right column shows printable ASCII, every other byte as `.`.
pub fn hex_dump(bytes: &[u8], first_offset: usize) -> Vec<String> {
    bytes
        .chunks(PER_LINE)
        .enumerate()
        .map(|(i, chunk)| {
            let offset = first_offset.saturating_add(i.saturating_mul(PER_LINE));
            let mut hex = String::with_capacity(PER_LINE * 3 + 1);
            for (j, b) in chunk.iter().enumerate() {
                if j == PER_LINE / 2 {
                    hex.push(' ');
                }
                hex.push_str(&format!("{b:02x} "));
            }
            let text: String = chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { char::from(b) } else { '.' }).collect();
            format!("{offset:08x}  {hex:<49} |{text}|")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_shows_offset_hex_and_printable_text() {
        let lines = hex_dump(b"\x1b[31mhi\r\n\x00\xff", 16);
        assert_eq!(lines, vec![format!("{:08x}  {:<49} |{}|", 16, "1b 5b 33 31 6d 68 69 0d  0a 00 ff ", ".[31mhi....")]);
    }

    #[test]
    fn long_input_spans_lines_of_sixteen() {
        let lines = hex_dump(&[b'a'; 40], 0);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("00000010  "));
        assert!(lines[2].starts_with("00000020  "));
    }

    #[test]
    fn rows_are_the_same_lines_as_the_whole_dump() {
        let mut buf = HexBuffer::default();
        buf.push(&[3u8; 70_000]);
        buf.push(b"tail");
        let all = buf.lines();
        assert_eq!(buf.line_count(), all.len());
        assert_eq!(buf.rows(5..9), all[5..9].to_vec());
        assert_eq!(buf.rows(all.len() - 1..all.len() + 5), all[all.len() - 1..].to_vec());
    }

    #[test]
    fn the_buffer_keeps_only_its_bound() {
        let mut buf = HexBuffer::default();
        for _ in 0..10 {
            buf.push(&[7u8; 20_000]);
        }
        assert_eq!(buf.len(), MAX_BYTES);
        buf.push(&vec![1u8; MAX_BYTES * 3]);
        assert_eq!(buf.len(), MAX_BYTES);
        // Offsets go on from the session's first byte, on lines of sixteen.
        let first = buf.lines().into_iter().next().unwrap();
        let offset = usize::from_str_radix(&first[..8], 16).unwrap();
        assert_eq!(offset % 16, 0);
        assert!(offset >= 200_000 + MAX_BYTES * 2);
    }
}
