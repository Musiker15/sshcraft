//! Design tokens: every colour and radius the interface draws with. Widgets never hard-code a
//! colour; they read [`Tokens::get`].

mod tokens;

pub use tokens::Tokens;
