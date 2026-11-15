//! Status dots: what a pane (and its tab) needs. Red: ended with an error; amber ring: waits for
//! input (the bell rang, or the last line asks for a password, a passphrase or yes/no); blue:
//! output not seen yet; green, yellow, grey: connected, connecting, ended.

use egui::{Color32, Painter, Pos2, Stroke};

use crate::theme::Tokens;

/// Most urgent first: the order is the ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dot {
    Error,
    Waiting,
    Unseen,
    Connected,
    Connecting,
    Ended,
}

impl Dot {
    /// The dot a tab shows for its panes.
    pub fn most_urgent(dots: impl IntoIterator<Item = Dot>) -> Option<Dot> {
        dots.into_iter().min()
    }

    fn color(self, t: &Tokens) -> Color32 {
        match self {
            Dot::Error => t.danger,
            Dot::Waiting | Dot::Connecting => t.warning,
            Dot::Unseen => t.info,
            Dot::Connected => t.success,
            Dot::Ended => t.text_dim,
        }
    }

    pub fn draw(self, painter: &Painter, center: Pos2, tokens: &Tokens) {
        let color = self.color(tokens);
        if self == Dot::Waiting {
            painter.circle_stroke(center, 3.5, Stroke::new(1.5, color));
        } else {
            painter.circle_filled(center, 3.5, color);
        }
    }
}

/// Words that mark a question for a secret wherever they stand in it.
const SECRETS: [&str; 9] = ["password", "passphrase", "pass phrase", "passwort", "kennwort", "mot de passe", "contraseña", "senha", "secret"];

/// Words a line ends with when it waits for a secret or a yes/no, in English, German, French,
/// Spanish and Portuguese.
const ASKS: [&str; 14] = [
    "password",
    "passphrase",
    "pin",
    "token",
    "code",
    "(yes/no)",
    "(yes/no/[fingerprint])",
    "passwort",
    "kennwort",
    "mot de passe",
    "contraseña",
    "senha",
    "código",
    "codigo",
];

/// Whether `line` (the line of the cursor) waits for an answer.
pub fn waiting_line(line: &str) -> bool {
    let line = line.trim_end().to_lowercase();
    let Some(question) = line.strip_suffix(':').or_else(|| line.strip_suffix('?')) else { return false };
    let full = question.trim_end();
    // A hint in brackets after the question: "Token (will be hidden):", "Key [None]:".
    let mut bare = full;
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some(rest) = bare.strip_suffix(close)
            && let Some(at) = rest.rfind(open)
        {
            bare = rest.get(..at).unwrap_or_default().trim_end();
        }
    }
    if SECRETS.iter().any(|s| bare.contains(s)) {
        return true;
    }
    [full, bare].iter().any(|question| asks(question))
}

fn asks(question: &str) -> bool {
    ASKS.iter().any(|ask| {
        let ask = ask.trim_start_matches('(');
        question.ends_with(ask.trim_end_matches(')'))
            || question.ends_with(&format!("({ask}"))
            || question.contains(&format!("{ask} for"))
            || question.contains(&format!("{ask} für"))
    }) || question.contains("(yes/no")
}
