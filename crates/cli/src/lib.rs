//! #264: the `herdsman` binary's crate: `herdsman_core` plus the server's `serve`, under the one `herdsman::` path.
#![warn(clippy::pedantic)]
pub use herdsman_core::*;
pub use herdsman_server::serve;
