use crate::hosts::On;
use crate::hygiene::NAMES_ENV;
use crate::runner::{argv, Out, Runner};
use anyhow::Result;
use std::path::Path;
use std::time::Duration;

/// The wrapper's check as a lane runs it, and the done check's name.
pub const CHECK: &str = "./.gatekeeper/run check";
const BODY: &str = "target/gatekeeper/pr-body.md";

/// The PR a done gate checks.
pub struct Pr<'a> {
    pub head: &'a str,
    pub body: &'a str,
}

/// Write `pr`'s body into `wt` on `on`'s host and run the wrapper once there, `names` on stdin as `DROVER_PRIVATE_NAMES`.
pub fn check(on: &On, wt: &Path, pr: &Pr, names: &[String], limit: Duration) -> Result<Out> {
    on.write(&wt.join(BODY), pr.body)?;
    let line = format!("cd \"$0\" && {NAMES_ENV}=\"$(cat)\" GATEKEEPER_HEAD=\"$1\" {CHECK} --pr-body {BODY}");
    let sh = argv(&["-c", &line, &wt.to_string_lossy(), pr.head]);
    on.run_for("sh", &sh, &names.join("\n"), limit)
}
