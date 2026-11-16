use std::path::{Path, PathBuf};
use std::process::Command;

fn git(args: &[&str]) -> String {
    let out = Command::new("git").args(args).output().ok();
    let out = out.filter(|o| o.status.success()).map(|o| o.stdout);
    String::from_utf8_lossy(&out.unwrap_or_default()).trim().to_string()
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn joined(p: &Path) -> String {
    if p.is_dir() {
        files(p).iter().map(|f| joined(f)).collect()
    } else {
        std::fs::read_to_string(p).unwrap_or_default()
    }
}

fn main() {
    let tail: String = files(Path::new("roles/usage"))
        .iter()
        .map(|p| format!("\n{}", joined(p)))
        .collect();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    std::fs::write(out.join("usage-tail.md"), tail).unwrap_or_default();
    println!("cargo:rerun-if-changed=roles/usage");
    println!("cargo:rustc-env=DROVER_SHA={}", git(&["rev-parse", "HEAD"]));
    let dir = git(&["rev-parse", "--git-dir"]);
    if !dir.is_empty() {
        println!("cargo:rerun-if-changed={dir}/logs/HEAD");
    }
}
