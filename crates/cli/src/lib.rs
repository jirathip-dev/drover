//! #264: the `herdsman` binary's crate: `herdsman_core` plus the server's `serve`, under the one `herdsman::` path.
#![warn(clippy::pedantic)]
pub use herdsman_core::*;
pub use herdsman_server::serve;

/// `herdsman_core::main_with` with `herdsman serve` wired in.
pub fn main_with(
    args: Vec<String>,
    ctx: &Ctx,
    r: &dyn runner::Runner,
    out: &mut dyn std::io::Write,
    stdin: &mut dyn std::io::Read,
) -> i32 {
    let _ = SERVE.set(Serve {
        serve: serve::serve,
        export: serve::export,
    });
    herdsman_core::main_with(args, ctx, r, out, stdin)
}
