//! The agent gate: what agents and the control channel may do. In R2.2 only the types; the
//! checks move here from the Tauri version (`agents/gate.rs`) with R2.7.

pub mod access;
pub mod scope;
pub mod view;
pub mod wire;

pub use access::{AgentAccess, AskRule, Present, Refusal, Resolve, Resolved};
pub use view::GateView;
