//! #264: `drover_core` plus the server's `serve`, under the one `drover::` path.
pub use drover_core::*;
pub use drover_server::{hooks, serve};
