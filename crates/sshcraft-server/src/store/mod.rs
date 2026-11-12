//! Storage behind the HTTP layer (Task 2 defines the contract).

pub trait Store: Send + Sync {}

/// Placeholder store until the SQLite backend exists.
pub struct NullStore;

impl Store for NullStore {}
