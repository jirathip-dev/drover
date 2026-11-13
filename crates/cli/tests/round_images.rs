//! #969: a design or verify card's image is its own round's PNG, through the service worker, after a later round wrote
//! other bytes at the same path; an old-style URL (no round) is the latest round's file.
mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// The real server on a free port, for the test's life.
fn serve(c: &'static drover::Ctx) -> (&'static Server<'static>, String) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let r: &'static Fake = Box::leak(Box::default());
    let s: &'static Server = Box::leak(Box::new(Server::new(c, r, Some(addr.clone()))));
    std::thread::spawn(move || {
        for mut st in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = std::io::Read::read(&mut st, &mut buf).unwrap_or(0);
            let _ = st.write_all(&s.respond(&String::from_utf8_lossy(&buf[..n]), c.now));
        }
    });
    (s, addr)
}

/// A browser's service worker (`tests/support/sw.mjs`): `get` fetches a URL through it.
struct Sw(std::process::Child, std::io::BufReader<std::process::ChildStdout>);
impl Sw {
    fn new(addr: &str) -> Self {
        let mut c = std::process::Command::new("node")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/sw.mjs"))
            .env("DROVER_BASE", format!("http://{addr}/"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("node (24+)");
        let out = std::io::BufReader::new(c.stdout.take().unwrap());
        Sw(c, out)
    }
    fn get(&mut self, url: &str) -> Vec<u8> {
        writeln!(self.0.stdin.as_mut().unwrap(), "{url}").unwrap();
        let mut line = String::new();
        self.1.read_line(&mut line).unwrap();
        let hex = line.trim();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}
impl Drop for Sw {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ev(c: &drover::Ctx, kind: &str, d: Value) {
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), kind, d)).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap().trim().into()
}

/// The old board's (needs.ts) image srcs in its needs-you strip.
fn needs_srcs(s: &Server, addr: &str, now: u64) -> Vec<String> {
    let [needs, ..] = parts(s, addr, "", now);
    needs
        .split("<img src=\"")
        .skip(1)
        .map(|x| x[..x.find('"').unwrap()].to_string())
        .collect()
}

/// Drover's screens' design card srcs (`/screens/cards`).
fn card_srcs(s: &Server, addr: &str, now: u64) -> Vec<String> {
    let res = s.respond(&format!("GET /screens/cards HTTP/1.1\r\nHost: {addr}\r\n\r\n"), now);
    let res = String::from_utf8_lossy(&res);
    let v: Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    let blocks = v
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["blocks"].as_array().unwrap().clone());
    let items = blocks
        .filter(|b| b["type"] == "images")
        .flat_map(|b| b["items"].as_array().unwrap().clone());
    items.map(|i| i["src"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_revised_designs_card_shows_its_own_rounds_png_through_the_service_worker() {
    let c: &'static drover::Ctx = Box::leak(Box::new(ctx(&tmpdir("round-design"), CONFIG)));
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(wt.join("docs/design/meter")).unwrap();
    git(&wt, &["init", "-q"]);
    let name = "docs/design/meter/1440.png";
    let png = wt.join(name);
    let (s, addr) = serve(c);
    let mut sw = Sw::new(&addr);
    let round = |k: u64, bytes: &[u8]| {
        std::fs::write(&png, bytes).unwrap();
        git(&wt, &["add", "-A"]);
        git(
            &wt,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                &format!("design {k}"),
            ],
        );
        let sha = git(&wt, &["rev-parse", "HEAD"]);
        let files = json!([{"name": name, "path": png}]);
        ev(
            c,
            "design-review",
            json!({"sha": sha, "files": files, "role": "design"}),
        );
        let id = format!("q{k}");
        let q = json!({"id": id, "question": "Design for #12 is ready: approve, revise (type a note) or reject?",
            "options": ["approve", "revise", "reject"], "recommend": null, "kind": "owner", "design": true, "files": files, "role": "design"});
        ev(c, "needs-decision", q);
        let (old, new) = (needs_srcs(s, &addr, c.now), card_srcs(s, &addr, c.now));
        assert_eq!((old.len(), new.len()), (1, 1), "round {k}: {old:?} {new:?}");
        (old[0].clone(), new[0].clone())
    };
    let (r1_old, r1_new) = round(1, b"\x89PNG round 1");
    assert_eq!(sw.get(&r1_old), b"\x89PNG round 1");
    assert_eq!(sw.get(&r1_new), b"\x89PNG round 1");
    ev(c, "decision", json!({"id": "q1", "choice": "revise", "by": "owner"}));
    let (r2_old, r2_new) = round(2, b"\x89PNG round 2");
    assert_eq!(sw.get(&r2_old), b"\x89PNG round 2", "the old board's card: {r2_old}");
    assert_eq!(sw.get(&r2_new), b"\x89PNG round 2", "Drover's screens' card: {r2_new}");
    // Round 1's card, read fresh from the server: still round 1's PNG, though the file now holds round 2's.
    let direct = |p: &str| {
        let p = if p.starts_with('/') {
            p.to_string()
        } else {
            format!("/{p}")
        };
        let res = s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {addr}\r\n\r\n"), c.now);
        res[res.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..].to_vec()
    };
    assert_eq!(direct(&r1_old), b"\x89PNG round 1");
    assert_eq!(direct(&r1_new), b"\x89PNG round 1");
    // An old-style URL (no round) is the latest ask's file, as before.
    assert_eq!(direct("/design/sendmeter/12/0"), b"\x89PNG round 2");
}

#[test]
fn a_later_verify_rounds_card_shows_its_own_screenshot_through_the_service_worker() {
    let c: &'static drover::Ctx = Box::leak(Box::new(ctx(&tmpdir("round-shot"), CONFIG)));
    let (s, addr) = serve(c);
    let mut sw = Sw::new(&addr);
    let round = |k: u64, bytes: &[u8]| {
        let lane = c.worktrees.join(format!("sendmeter/verify-12-r{k}"));
        std::fs::create_dir_all(lane.join("shots")).unwrap();
        std::fs::write(lane.join("shots/home-390.png"), bytes).unwrap();
        let path = lane.canonicalize().unwrap().join("shots/home-390.png");
        ev(
            c,
            "spawn",
            json!({"role": "verify", "stage": "verify", "check": true, "agent": format!("verify-{k}"), "worktree": lane}),
        );
        let files = json!([{"name": "shots/home-390.png", "path": path}]);
        let sha = k.to_string().repeat(40);
        ev(
            c,
            "verdict",
            json!({"verdict": "FAIL", "stage": "verify", "sha": sha, "files": files, "body": "FAIL\n"}),
        );
        ev(c, "needs-human", json!({"reason": "verify FAIL"}));
        let srcs = needs_srcs(s, &addr, c.now);
        assert_eq!(srcs.len(), 1, "round {k}: {srcs:?}");
        srcs[0].clone()
    };
    let r1 = round(1, b"\x89PNG verify 1");
    assert_eq!(sw.get(&r1), b"\x89PNG verify 1");
    let r2 = round(2, b"\x89PNG verify 2");
    assert_eq!(sw.get(&r2), b"\x89PNG verify 2", "{r2}");
}
