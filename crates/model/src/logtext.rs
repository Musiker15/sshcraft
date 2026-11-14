//! Terminal output as plain text: escape sequences removed, line endings and overwrites
//! resolved the way a reader expects. Used for searching and exporting session logs.

/// `bytes` as text without colors, titles and cursor movements.
pub fn plain_text(bytes: &[u8]) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Text,
        Escape,
        /// One more character follows (`ESC (` charset designations and the like).
        EscapeArg,
        Csi,
        /// OSC, DCS, SOS, PM, APC: until BEL or ST.
        Command,
        CommandEscape,
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut line: Vec<char> = Vec::new();
    let mut carriage_return = false;
    let mut state = State::Text;
    for c in text.chars() {
        state = match state {
            State::Escape => match c {
                '[' => State::Csi,
                ']' | 'P' | 'X' | '^' | '_' => State::Command,
                '(' | ')' | '*' | '+' | '-' | '.' | '/' | '#' | '%' | ' ' => State::EscapeArg,
                _ => State::Text,
            },
            State::EscapeArg => State::Text,
            State::Csi if ('\u{40}'..='\u{7e}').contains(&c) => State::Text,
            State::Csi => State::Csi,
            State::Command => match c {
                '\u{7}' => State::Text,
                '\u{1b}' => State::CommandEscape,
                _ => State::Command,
            },
            State::CommandEscape => State::Text,
            State::Text => {
                match c {
                    '\u{1b}' => {}
                    '\r' => carriage_return = true,
                    '\n' => {
                        out.extend(line.drain(..));
                        out.push('\n');
                        carriage_return = false;
                    }
                    '\u{8}' => {
                        line.pop();
                    }
                    c if c == '\t' || !c.is_control() => {
                        if carriage_return {
                            line.clear();
                            carriage_return = false;
                        }
                        line.push(c);
                    }
                    _ => {}
                }
                if c == '\u{1b}' { State::Escape } else { State::Text }
            }
        };
    }
    out.extend(line);
    out
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn colors_are_removed() {
        assert_eq!(plain_text(b"\x1b[31mred\x1b[0m done"), "red done");
        assert_eq!(plain_text(b"\x1b[1;38;5;208mx\x1b[m"), "x");
    }

    #[test]
    fn titles_end_at_bel_or_st() {
        assert_eq!(plain_text(b"\x1b]0;user@host\x07$ ls"), "$ ls");
        assert_eq!(plain_text(b"\x1b]2;title\x1b\x5c$ pwd"), "$ pwd");
        assert_eq!(plain_text(b"a\x1bPq#0;1\x1b\x5cb"), "ab");
    }

    #[test]
    fn crlf_is_one_line_break() {
        assert_eq!(plain_text(b"a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn carriage_return_overwrites_the_line() {
        assert_eq!(plain_text(b"50%\r100%\r\n"), "100%\n");
    }

    #[test]
    fn backspace_removes_the_character_before() {
        assert_eq!(plain_text(b"ab\x08c"), "ac");
        assert_eq!(plain_text("äb\x08\x08o".as_bytes()), "o");
    }

    #[test]
    fn other_controls_vanish_tabs_stay() {
        assert_eq!(plain_text(b"a\x07b\tc\x00"), "ab\tc");
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        assert_eq!(plain_text(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn charset_designation_takes_one_more_byte() {
        assert_eq!(plain_text(b"\x1b(Bok\x1b=x"), "okx");
    }
}
