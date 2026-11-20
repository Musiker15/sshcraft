//! SFTP: file browsing and transfers over an SSH connection (S1).

pub mod edits;
pub mod manager;
pub mod names;
pub mod transfers;

pub use manager::{Count, EntryView, ListingView, SftpId, SftpManager};

#[cfg(test)]
mod tests;
