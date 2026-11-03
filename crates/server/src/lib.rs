//! #264: `herdsman serve`, the phone page, over `herdsman_core` (never the cli crate).
#![warn(clippy::pedantic)]
// Same allow-list as herdsman_core (docs/architecture.md, Code rules).
#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::format_collect,
    clippy::many_single_char_names
)]
// serve.rs moved as is: its `crate::` paths resolve through this re-export.
pub use herdsman_core::*;
pub mod serve;
