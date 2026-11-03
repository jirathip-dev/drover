//! #235: the git sha this binary is built from, for `herdsman --version` and the remote hosts' version check.
use std::process::Command;

fn git(args: &[&str]) -> String {
    let out = Command::new("git").args(args).output().ok();
    let out = out.filter(|o| o.status.success()).map(|o| o.stdout);
    String::from_utf8_lossy(&out.unwrap_or_default()).trim().to_string()
}

fn main() {
    println!("cargo:rustc-env=HERDSMAN_SHA={}", git(&["rev-parse", "HEAD"]));
    let dir = git(&["rev-parse", "--git-dir"]);
    if !dir.is_empty() {
        println!("cargo:rerun-if-changed={dir}/logs/HEAD");
    }
}
