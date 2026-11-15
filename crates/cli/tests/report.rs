mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
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
    assert!(post(&s, &body, "").starts_with("HTTP/1.1 401 "));
    assert!(reports(&c).is_empty() && !heard.exists());
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
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
    let id = b["id"].as_str().unwrap();
    let (code, out) = run(&c, &Fake::default(), &["report"]);
    assert!(code == 0 && out.contains(id) && out.contains(&path), "{out}");
    let (code, out) = run(&c, &Fake::default(), &["report", id]);
    assert!(
        code == 0 && out.contains("row overlaps the badge") && out.contains("\"tab\": \"lanes\""),
        "{out}"
    );
}
