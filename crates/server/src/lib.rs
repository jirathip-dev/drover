// #264: serve.rs moved as is; its `crate::` paths resolve through this re-export.
pub use herdsman_core::*;
pub mod api;
pub mod hooks;
pub mod lifeboat;
pub mod serve;

/// #780: the frozen board (#552, serve.rs and serve/) still names the orchestrator module by its old name.
pub use herdsman_core::orchestrator as conductor;
