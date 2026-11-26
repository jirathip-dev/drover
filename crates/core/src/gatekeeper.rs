use crate::heavy::Heavy;
use crate::hosts::On;
use crate::hygiene::NAMES_ENV;
use crate::runner::{argv, Out, Runner};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The wrapper's check as a lane runs it, and the done check's name.
pub const CHECK: &str = "./.gatekeeper/run check";
const BODY: &str = "target/gatekeeper/pr-body.md";

/// The PR a done gate checks.
pub struct Pr<'a> {
    pub head: &'a str,
    pub body: &'a str,
}

/// The wrapper's path in the repo at `dir`.
pub fn wrapper(dir: &Path) -> PathBuf {
    dir.join(".gatekeeper/run")
}

const ENV: [&str; 3] = [NAMES_ENV, "GATEKEEPER_HEAD", "GATEKEEPER_PR_BODY"];

/// Write `pr`'s body into `wt` on `on`'s host and run the wrapper once there, `names` on stdin as `DROVER_PRIVATE_NAMES`; with `via` (a lane's heavy rules and `drover`'s path) on its `cargo` route's build machines.
pub fn check(
    on: &On,
    wt: &Path,
    pr: &Pr,
    names: &[String],
    limit: Duration,
    via: Option<(&Heavy, &str)>,
) -> Result<Out> {
    on.write(&wt.join(BODY), pr.body)?;
    let run = format!("{CHECK} --pr-body {BODY}");
    let run = match via {
        Some((h, exe)) => {
            let mut h = h.clone();
            (h.routes.values_mut()).for_each(|r| r.env.extend(ENV.map(String::from)));
            let made = format!("mkdir -p target/gatekeeper && printf %s \"$GATEKEEPER_PR_BODY\" > {BODY} && {run}");
            h.gate(exe, &made, &wt.to_string_lossy())
        }
        None => run,
    };
    let line =
        format!("cd \"$0\" && export {NAMES_ENV}=\"$(cat)\" GATEKEEPER_HEAD=\"$1\" GATEKEEPER_PR_BODY=\"$2\" && {run}");
    let sh = argv(&["-c", &line, &wt.to_string_lossy(), pr.head, pr.body]);
    on.run_for("sh", &sh, &names.join("\n"), limit)
}

/// The merge check's cap command for the repo at `dir`: the wrapper's `core/cap` gate, when the repo holds it.
pub fn cap(dir: &Path) -> Option<String> {
    wrapper(dir).exists().then(|| format!("{CHECK} --gate core/cap"))
}
