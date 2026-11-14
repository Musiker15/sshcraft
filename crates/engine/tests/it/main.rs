//! Integration tests of the engine through its public interface: random command sequences, the
//! stable command ids, and where every command of the Tauri version went.

// Tests report by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod command_ids;
mod command_sequence_fuzz;
mod tauri_mapping;
