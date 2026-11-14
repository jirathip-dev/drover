mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const CHROME: &str = r#"#!/usr/bin/env bash
for a; do case $a in http*) url=$a ;; esac; done
case $url in *"audit=$HANG&"*) exec sleep 30 ;; esac
for a; do case $a in --screenshot=*) : > "${a#*=}" ;; esac; done
case $url in *"audit=$BAD&check=1"*) echo '<pre id="audit">{"outliers":["[x]: 20 px tall"]}</pre>' ;;
  *check=1*) echo '<pre id="audit">{"outliers":[]}</pre>' ;; esac
"#;

fn audit(hang: &str, bad: &str) -> (Option<i32>, String, String, String) {
    let d = tmpdir("ui-hang");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(d.join("site")).unwrap();
    std::fs::write(d.join("site/index.html"), "ok").unwrap();
    for (name, body) in [("chrome", CHROME), ("node", "#!/bin/sh\nexit 0\n")] {
        std::fs::write(bin.join(name), body).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("bash")
        .arg(root.join("scripts/ui-audit.sh"))
        .arg(d.join("site"))
        .arg(d.join("shots"))
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
        .env("CHROME", bin.join("chrome"))
        .env("UI_AUDIT_PORT", port.to_string())
        .env("UI_AUDIT_TIMEOUT", "1")
        .env("UI_AUDIT_VIEWS", "queue config")
        .env("HANG", hang)
        .env("BAD", bad)
        .output()
        .unwrap();
    let read = |f: &str| std::fs::read_to_string(d.join("shots").join(f)).unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code(), out, read("hangs.txt"), read("flaky.txt"))
}

#[test]
fn a_view_that_never_settles_is_retried_twice_then_flaky_and_ui_stays_green() {
    let (code, out, hangs, flaky) = audit("queue", "none");
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(flaky, "queue --screenshot\nqueue --dump-dom\n", "{out}");
    let tries: Vec<_> = hangs.lines().collect();
    assert_eq!(
        tries,
        [1, 2, 3]
            .iter()
            .map(|n| format!("queue --screenshot: try {n} hung"))
            .chain([1, 2, 3].iter().map(|n| format!("queue --dump-dom: try {n} hung")))
            .collect::<Vec<_>>(),
        "{out}"
    );
    assert!(out.contains("queue: hung 3 times, not checked (ui-flaky)"), "{out}");
    assert!(out.contains("config: 0 controls, 0 outliers"), "{out}");
}

#[test]
fn a_real_outlier_still_fails_ui_beside_a_hung_view() {
    let (code, out, _, flaky) = audit("queue", "config");
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("config: 0 controls, 1 outliers\n  [x]: 20 px tall"),
        "{out}"
    );
    assert_eq!(flaky, "queue --screenshot\nqueue --dump-dom\n", "{out}");
}
