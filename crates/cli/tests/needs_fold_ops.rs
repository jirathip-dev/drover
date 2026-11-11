mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

fn board(name: &str) -> (herdsman::Ctx, String, String) {
    let c = ctx(&tmpdir(name), CONFIG);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr workspace list", r#"{"result":{"workspaces":[]}}"#);
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "add", "12", "20"]).0, 0);
    assert_eq!(run(&c, &r, &["pause", "sendmeter"]).0, 0);
    let ev = |ts, n, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
    ev(
        10,
        12,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    for (i, sha) in ["2", "3", "4", "5"].iter().enumerate() {
        ev(
            20 + i as u64,
            12,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha.repeat(40)}),
        );
    }
    ev(
        30,
        12,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "review rounds ran out (4/3)"}),
    );
    let q = json!({"id": "q1", "question": "Keep the old flag?", "options": ["yes", "no"], "recommend": 1, "kind": "owner"});
    ev(40, 20, "needs-decision", q);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fake = Fake::default();
    let s = herdsman::serve::Server::new(&c, &fake, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", now);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &r, l, false));
    (c, url, cookie.trim().trim_start_matches("Cookie: ").to_string())
}

const PRE: &str = "window.posts = []; const F = fetch; \
    window.fetch = (u, o) => { if (o && o.body instanceof URLSearchParams) posts.push(String(o.body)); return F(u, o); }";

const TAPS: &str = "window.ts = []; const t0 = toast; toast = (m, s) => (ts.push(m), t0(m, s)); (function go() { \
    const nh = () => $('#needs details.ask[data-n=\"12\"] summary button[form]'), ask = $('#needs details.ask:not([data-n]) summary button[form]'); \
    if (!nh() || !ask) return setTimeout(go, 20); \
    const b = nh(), said = r => ts.some(m => r.test(m)) ? 1 : 0; \
    b.value = ''; b.click(); b.value = 'extend'; \
    const n0 = posts.length, none = [n0 === 0 ? 1 : 0, said(/^error: .*#12/), nh() === b && !$('#needs .pend') ? 1 : 0]; \
    ask.click(); \
    setTimeout(() => { nh().click(); \
      (function done(end) { if (!said(/#12: extend/) && performance.now() < end) return setTimeout(done, 50, end); \
        const p = posts.slice(n0).map(x => new URLSearchParams(x)), one = (q, k) => q ? q.getAll(k).join() : ''; \
        window.late = [p.length, ...none, \
          one(p[0], 'op') === 'answer' && one(p[0], 'id') === 'q1' && one(p[0], 'choice') === '1' ? 1 : 0, \
          one(p[1], 'op') === 'extend' && one(p[1], 'issue') === '12' ? 1 : 0, \
          said(/answered q1/), said(/#12: extend/)]; })(performance.now() + 5000); }, 1500); })()";

#[test]
fn folded_main_buttons_post_their_op_in_webkit() {
    if !cfg!(target_os = "macos") || std::process::Command::new("swift").arg("--version").output().is_err() {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let _lock = ui_lock();
    let shots = std::env::var("HERDSMAN_SNAPSHOTS").map_or_else(|_| tmpdir("needs871-png"), std::path::PathBuf::from);
    for w in ["390", "1440"] {
        let (c, url, cookie) = board(&format!("needs871-{w}"));
        let png = shots.join(format!("needs-fold-ops-{w}.png"));
        let o = swift()
            .env("HERDSMAN_COOKIE", &cookie)
            .env("HERDSMAN_WAIT", "9")
            .env("HERDSMAN_PRE", PRE)
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift"),
                &url,
                w,
                "queue",
                "scene",
                png.to_str().unwrap(),
                TAPS,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
        let m: Vec<f64> = out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect();
        assert_eq!(
            m,
            [2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            "{w}: posts, no-op sent nothing, toast names #12, card stayed, answer body, extend body, answered toast, extend toast"
        );
        let evs = read(&c.state).unwrap();
        assert!(
            evs.iter().any(|e| e.kind == "decision" && e.str("id") == Some("q1")),
            "{w}: q1 answered"
        );
        assert_eq!(
            evs.iter().filter(|e| e.kind == "round-extended").count(),
            1,
            "{w}: one round-extended"
        );
    }
}
