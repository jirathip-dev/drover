//! #363: the board's report mode. A paired phone's bundle lands under `state/reports/<id>/` and the notifier hears its
//! path; an unpaired one is refused and nothing is kept. Gate 2: report mode at 390 px in WebKit against a real `serve`.
mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::path::Path;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
/// A 1×1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn post(s: &Server, body: &str, extra: &str) -> String {
    let r = format!(
        "POST /report HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

/// The ctx with a `merged` notifier appending to `<dir>/heard.txt`: a report reaches it all the same.
fn setup(name: &str) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let heard = d.join("heard.txt");
    let cfg = format!(
        "{CONFIG}\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"merged\"]\n",
        heard.display()
    );
    (ctx(&d, &cfg), heard)
}

fn reports(c: &drover::Ctx) -> Vec<std::path::PathBuf> {
    let dir = c.state.parent().unwrap().join("reports");
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect()
}

#[test]
fn a_paired_report_is_kept_and_named_to_the_notifier() {
    let (c, heard) = setup("report");
    let f = Fake::default();
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let html = format!("<div class=lanes>{}</div>", "é".repeat(5000));
    let body = format!(
        "html={}&styles={}&ctx={}&note={}&png={}",
        enc(&html),
        enc(r#"{"color":"rgb(1, 2, 3)"}"#),
        enc(r#"{"tab":"lanes","theme":"farm","live":42}"#),
        enc("row overlaps the badge\nsecond line dropped"),
        enc(&format!("data:image/png;base64,{PNG}"))
    );
    // Unpaired: refused, nothing kept, nothing heard.
    assert!(post(&s, &body, "").starts_with("HTTP/1.1 401 "));
    assert!(reports(&c).is_empty() && !heard.exists());
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    // Not a PNG: refused, nothing kept.
    let res = post(&s, &body.replace("iVBOR", "AAAA"), &cookie);
    assert!(res.starts_with("HTTP/1.1 409 ") && reports(&c).is_empty(), "{res}");
    let res = post(&s, &body, &cookie);
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    let dir = reports(&c).pop().unwrap();
    let b: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("bundle.json")).unwrap()).unwrap();
    assert!(res.contains(&dir.join("bundle.json").display().to_string()), "{res}");
    assert_eq!(
        b["html"].as_str().unwrap().len(),
        8191,
        "trimmed to 8 KB on a char boundary"
    );
    assert_eq!(
        (
            &b["styles"]["color"],
            &b["context"]["tab"],
            &b["context"]["live"],
            &b["note"]
        ),
        (
            &json!("rgb(1, 2, 3)"),
            &json!("lanes"),
            &json!(42),
            &json!("row overlaps the badge")
        )
    );
    assert_eq!(b["device"], json!(drover::devices::load(&c)[0].label));
    assert!(b["build"].as_str().unwrap().contains('('), "names the build sha");
    assert!(std::fs::read(dir.join("shot.png")).unwrap().starts_with(b"\x89PNG"));
    let e = read(&c.state).unwrap().to_vec().pop().unwrap();
    assert_eq!((e.kind.as_str(), e.fleet.as_str()), ("report", "sendmeter"));
    let path = dir.join("bundle.json").display().to_string();
    assert!(
        std::fs::read_to_string(&heard).unwrap().contains(&path),
        "the notifier hears the path"
    );
    // The conductor reads it back.
    let id = b["id"].as_str().unwrap();
    let (code, out) = run(&c, &Fake::default(), &["report"]);
    assert!(code == 0 && out.contains(id) && out.contains(&path), "{out}");
    let (code, out) = run(&c, &Fake::default(), &["report", id]);
    assert!(
        code == 0 && out.contains("row overlaps the badge") && out.contains("\"tab\": \"lanes\""),
        "{out}"
    );
}

fn board(c: &drover::Ctx) -> (String, String, FakeShed) {
    let sp = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "worktree": "/wt/x"});
    append(&c.state, &Event::new(900_000, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let sh = FakeShed::new();
    sh.attach(c);
    sh.lane("sendmeter-impl-12", "working", "/x");
    sh.screen("sendmeter-impl-12", "⏺ working\n");
    let r = Fake::default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cookie = pair(&Server::new(c, &Fake::default(), Some(ADDR.into())), c, "Safari", now);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    (url, cookie.trim().trim_start_matches("Cookie: ").to_string(), sh)
}

/// The page in WebKit at 390 px: `js` once the lane row is in, `window.late` read 1.5 s later.
fn page(url: &str, cookie: &str, js: &str, png: &Path) -> Vec<f64> {
    let js = format!(
        "(function go() {{ const t = document.querySelector('#lanes .lane[data-agent]'); if (!t) return setTimeout(go, 20); {js} }})()"
    );
    let o = webview()
        .env("DROVER_COOKIE", cookie)
        .env("DROVER_WAIT", "1.5")
        .args([url, "390", "lanes", "scene", png.to_str().unwrap(), &js])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

#[test]
fn report_mode_in_webkit_outlines_confirms_and_sends_a_bundle() {
    if no_webkit() {
        return;
    }
    let c = ctx(&tmpdir("report-wk"), CONFIG);
    let (url, cookie, _sh) = board(&c);
    let shots = std::env::var("DROVER_SNAPSHOTS").map_or_else(|_| tmpdir("report-png"), std::path::PathBuf::from);
    // `/report` from the reply box's menu, a tap (outline), a second tap (the note box), a note: the 390 px shot.
    // The taps come 400 ms on, once the sheet has slid away.
    let tap = |then: &str| {
        format!(
            "open(t); ta.value = '/re'; ta.oninput(); cmds.querySelector('li').click(); setTimeout(() => {{ t.click(); \
             const one = document.querySelectorAll('.repo').length, box = !!document.querySelector('.repf'); t.click(); \
             const f = document.querySelector('.repf'); f.note.value = 'badge overlaps the row'; {then} }}, 400);"
        )
    };
    let shown = tap(
        "window.late = [one, +box, document.querySelectorAll('.repf').length, getComputedStyle(t).outlineStyle === 'solid' ? 1 : 0, \
         document.querySelector('.repf').getBoundingClientRect().bottom, parseFloat(getComputedStyle(f.note).fontSize)];"
    );
    let v = page(&url, &cookie, &shown, &shots.join("ui-report.png"));
    assert_eq!(
        v,
        [1.0, 0.0, 1.0, 1.0, 844.0, 16.0],
        "outlined, no box after one tap, the box after two, at the bottom, 16 px"
    );
    assert!(reports(&c).is_empty(), "nothing sent before Send");
    // The toast is the way out: a fake clock fires the 4 s fade at once; the toast stays and a tap on it ends the mode.
    let stays = "const T = setTimeout; window.setTimeout = (f, d) => (d >= 4000 ? (f(), 0) : T(f, d)); \
         open(t); ta.value = '/re'; ta.oninput(); cmds.querySelector('li').click(); window.setTimeout = T; \
         T(() => { const m = document.querySelector('.msg.g'), on = +document.body.classList.contains('rep'); \
         m && m.click(); window.late = [+!!m, on, +document.body.classList.contains('rep')]; }, 600);";
    let v = page(&url, &cookie, stays, &shots.join("ui-report-toast.png"));
    assert_eq!(
        v,
        [1.0, 1.0, 0.0],
        "the toast outlives its fade; a tap on it stops report mode"
    );
    // Send: the bundle lands; its PNG is the row plus 24 px at the device's ratio.
    let send = tap(
        "const r = t.getBoundingClientRect(); f.requestSubmit(); \
         window.late = [Math.ceil((Math.min(innerWidth, r.right + 24) - Math.max(0, r.left - 24)) * devicePixelRatio), \
         Math.ceil((Math.min(innerHeight, r.bottom + 24) - Math.max(0, r.top - 24)) * devicePixelRatio), +document.body.classList.contains('rep')];"
    );
    let v = page(&url, &cookie, &send, &shots.join("ui-report-sent.png"));
    let dir = reports(&c).pop().expect("a report kept");
    let b: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("bundle.json")).unwrap()).unwrap();
    assert!(
        b["html"].as_str().unwrap().contains("data-agent=\"sendmeter-impl-12\""),
        "{b}"
    );
    assert!(
        !b["html"].as_str().unwrap().contains("repo"),
        "the outline class is not in the markup"
    );
    assert_eq!(
        (&b["note"], &b["context"]["tab"], &b["context"]["viewport"][0]),
        (&json!("badge overlaps the row"), &json!("lanes"), &json!(390))
    );
    assert!(
        b["styles"]["background-color"].is_string() && b["context"]["live"].is_number(),
        "{b}"
    );
    let png = std::fs::read(dir.join("shot.png")).unwrap();
    let dim = |i: usize| f64::from(u32::from_be_bytes(png[i..i + 4].try_into().unwrap()));
    assert!(png.starts_with(b"\x89PNG") && png.len() > 1000, "a drawn PNG");
    assert_eq!(
        [dim(16), dim(20), v[2]],
        [v[0], v[1], 0.0],
        "the crop's size; report mode ends"
    );
    // Unpaired: the same taps send nothing.
    let n = reports(&c).len();
    page(&url, "x=1", &send, &shots.join("ui-report-unpaired.png"));
    assert_eq!(reports(&c).len(), n, "nothing kept from an unpaired device");
}
