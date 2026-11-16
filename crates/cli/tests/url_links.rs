mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::json;

#[test]
fn an_events_row_with_a_pr_is_a_real_link_in_webkit() {
    if no_webkit() {
        return;
    }
    let d = tmpdir("url-links");
    let c = ctx(&d, CONFIG);
    let merged = json!({"into": "staging", "pr": "161"});
    append(
        &c.state,
        &Event::new(drover::now() - 60, "sendmeter", Some(12), "merged", merged),
    )
    .unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(&c, &Fake::default(), l, false));
    let row = "window.late = new Promise(ok => { const t = Date.now(), f = () => { \
        const r = [...document.querySelectorAll('.row')].find(r => r.textContent.includes('#161')); \
        if (r) ok([r.tagName === 'A', r.getAttribute('href'), r.target === '_blank', r.rel === 'noopener']); \
        else if (Date.now() - t > 5000) ok([-1]); else setTimeout(f, 50); }; f(); })";
    let pr = "https://github.com/jirathip-dev/sendmeter/pull/161";
    let js = format!("{row}.then(v => v.map(x => typeof x === 'string' ? x === '{pr}' : x))");
    let o = webview()
        .args([&format!("http://{addr}/#events"), "390", "-", "-"])
        .arg(d.join("events.png"))
        .arg(js)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let v: Vec<&str> = out.split_whitespace().collect();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v.get(2..),
        Some(&["1", "1", "1", "1"][..]),
        "the PR row is <a href=…/pull/161 target=_blank rel=noopener>: {out}"
    );
}
