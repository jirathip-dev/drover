mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const LANE: &str = "sendmeter-impl-12";

const STEPS: &str = r#"Object.defineProperty(window, 'late', {get: () => (async () => {
  const wait = async f => { for (let i = 0; i < 200; i++) { const x = f(); if (x) return x; await new Promise(r => setTimeout(r, 50)); } return null; };
  const open = t => !document.getElementById('sheet').hidden && document.querySelector('#sheet .sh b')?.textContent === t;
  const row = await wait(() => document.querySelector('main [data-id="q-sendmeter-12"]'));
  if (!row) return [0, 0, 0];
  row.click();
  const lane = await wait(() => open('#12') && document.querySelector('#sheet [data-id="lane"]'));
  if (!lane) return [1, 0, 0];
  lane.click();
  const back = await wait(() => open('sendmeter-impl-12') && document.querySelector('#sheet .sh .bk'));
  if (!back) return [1, 1, 0];
  back.click();
  return [1, 1, (await wait(() => open('#12') && document.querySelector('#sheet [data-id="lane"]'))) ? 1 : 0];
})()})"#;

#[test]
fn a_live_rows_tap_opens_its_issue_sheet_whose_lane_link_opens_the_transcript_and_back_returns() {
    if no_webkit() {
        return;
    }
    let d = tmpdir("issue-sheet-webkit");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let now = drover::now();
    let sp =
        json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt, "branch": "issue-12", "runtime": "shed"});
    append(&c.state, &Event::new(now - 600, "sendmeter", Some(12), "spawn", sp)).unwrap();
    sh.lane(LANE, "working", &wt);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    let png = d.join("sheet.png");
    let o = webview()
        .env("DROVER_COOKIE", ck.trim().trim_start_matches("Cookie: "))
        .env("DROVER_TAB", "queue")
        .args([&format!("http://{addr}/"), "390", "-", "-"])
        .arg(&png)
        .arg(STEPS)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let steps: Vec<&str> = out.split_whitespace().skip(2).collect();
    assert!(
        o.status.success() && steps == ["1", "1", "1"],
        "row → issue sheet, [lane] → transcript, back → issue sheet: {out} {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
