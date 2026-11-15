mod common;
use common::*;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

fn serve() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut b = [0; 2048];
            let n = s.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..n]).into_owned();
            let path = req.split(' ').nth(1).unwrap_or("/").to_string();
            let page = |body: &str| format!("<!doctype html><title>t</title><body style=\"margin:0\">{body}</body>");
            let (status, body) = match path.as_str() {
                "/error" => ("200 OK", page("<p>ok</p><script>console.error('meter: boom')</script>")),
                "/fetch500" => ("200 OK", page("<p>ok</p><script>fetch('/api')</script>")),
                "/api" => ("500 Internal Server Error", "down".into()),
                "/red" => ("200 OK", page("<div style=\"height:100vh;background:#f00\"></div>")),
                "/blue" => ("200 OK", page("<div style=\"height:100vh;background:#00f\"></div>")),
                _ => ("404 Not Found", String::new()),
            };
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{addr}")
}

fn verify(chrome: &std::ffi::OsStr, url: &str, out: &Path, extra: &[&str]) -> (i32, String) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/verify-browser.mjs");
    let o = Command::new("node")
        .arg(script)
        .arg(url)
        .arg(out)
        .args(["--name", "page", "--wait", "300"])
        .args(extra)
        .env("CHROME", chrome)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap_or(-1), text)
}

#[test]
fn a_console_error_fails_unless_known_noise_and_a_500_fails() {
    let Some(chrome) = std::env::var_os("CHROME") else {
        return eprintln!("skipping: no CHROME here");
    };
    let (d, base) = (tmpdir("verify-browser"), serve());
    let (code, out) = verify(&chrome, &format!("{base}/error"), &d.join("a"), &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL console error: meter: boom"), "{out}");
    let log = std::fs::read_to_string(d.join("a/page-console.log")).unwrap();
    assert_eq!(log, "console error: meter: boom\n");
    assert!(d.join("a/page-1440.png").is_file() && d.join("a/page-390.png").is_file());
    assert!(std::fs::read_to_string(d.join("a/page-a11y.json"))
        .unwrap()
        .contains("\"issues\""));

    let recipe = d.join("verify.md");
    std::fs::write(
        &recipe,
        "# recipe\n\n## Known noise\n\n- `meter: boom`\n\n## Drive\n\n- x\n",
    )
    .unwrap();
    let r = recipe.to_string_lossy();
    let (code, out) = verify(&chrome, &format!("{base}/error"), &d.join("b"), &["--recipe", &r]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("FAIL"), "{out}");
    let log = std::fs::read_to_string(d.join("b/page-console.log")).unwrap();
    assert_eq!(log, "console error: meter: boom [known noise]\n");

    let (code, out) = verify(&chrome, &format!("{base}/fetch500"), &d.join("c"), &["--recipe", &r]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains(&format!("FAIL network 500: GET {base}/api")), "{out}");
    let log = std::fs::read_to_string(d.join("c/page-network.log")).unwrap();
    assert_eq!(log, format!("network 500: GET {base}/api\n"));
}

#[test]
fn a_shot_over_the_threshold_from_its_approved_png_fails_with_its_diff_image() {
    let Some(chrome) = std::env::var_os("CHROME") else {
        return eprintln!("skipping: no CHROME here");
    };
    let (d, base) = (tmpdir("verify-diff"), serve());
    let (approved, a) = (d.join("approved"), d.join("approved").to_string_lossy().into_owned());
    assert_eq!(verify(&chrome, &format!("{base}/blue"), &approved, &[]).0, 0);

    let (code, out) = verify(&chrome, &format!("{base}/blue"), &d.join("same"), &["--approved", &a]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("0.000 % of pixels differ"), "{out}");

    let (code, out) = verify(&chrome, &format!("{base}/red"), &d.join("red"), &["--approved", &a]);
    assert_eq!(code, 1, "{out}");
    let diff = d.join("red/page-1440-diff.png");
    let line = out
        .lines()
        .find(|l| l.starts_with("FAIL diff:"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.contains(&diff.to_string_lossy().into_owned()) && line.ends_with("over 0.1 %"),
        "{out}"
    );
    assert!(std::fs::read(&diff).unwrap().starts_with(b"\x89PNG"));
    let (shot, ok) = (d.join("red/page-390.png"), approved.join("page-390.png"));
    let o = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/verify-browser.mjs"))
        .arg("diff")
        .args([&shot, &ok])
        .arg("100")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
}
