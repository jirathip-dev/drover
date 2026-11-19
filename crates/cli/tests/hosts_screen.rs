mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const RECIPE: &str =
    "key_env = \"FAKE_CLOUD_KEY\"\nstart = \"fakecloud start {name}\"\nstop = \"fakecloud stop {name}\"\n\
status = \"fakecloud status {name}\"\naddress = \"fakecloud address {name}\"\ndestroy = \"fakecloud destroy {name}\"\n\
[rate_usd_hour]\nsmall = 1.0\n";

fn fleet(name: &str) -> (drover::Ctx, String) {
    let hosts = "[proxy]\nport = 9\ntoken-file = \"/nonexistent/drover/proxy\"\n\n[hosts.mini]\n\n\
[hosts.cloud1]\nmachine = \"cloud1\"\ncloud = \"fake\"\nsize = \"small\"\nruntime = \"shed-acp\"\n";
    let c = ctx(&tmpdir(name), &format!("{CONFIG}\n{hosts}"));
    std::fs::create_dir_all(c.config.with_file_name("recipes")).unwrap();
    std::fs::write(c.config.with_file_name("recipes/fake.toml"), RECIPE).unwrap();
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let ev = |n: u64, kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(n), kind, d)).unwrap();
    ev(
        12,
        "spawn",
        json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": "shed"}),
    );
    ev(12, "merged", json!({"pr": "1", "merge_sha": "m", "into": "staging"}));
    ev(
        14,
        "spawn",
        json!({"role": "impl", "agent": "impl-14", "worktree": "/w/issue-14", "pane": "impl-14", "host": "cloud1", "machine": "cloud1"}),
    );
    (c, wt)
}

fn runner(wt: &str) -> Fake {
    let lsof = format!("p101\ncnode\nfcwd\nn{wt}/web\np102\ncvite\nfcwd\nn{wt}\n");
    Fake::default().reply("lsof", &lsof).reply("kill", "")
}

fn get(s: &Server, path: &str) -> (u16, Value) {
    let r = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn post(s: &Server, ck: &str, action: &str, values: &Value) -> (u16, Value) {
    let body = json!({"screen": "hosts", "block": "hosts-list", "action": action, "values": values}).to_string();
    let r = format!(
        "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{ck}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn swipe(v: &Value, host: &str) -> Vec<(String, Value)> {
    let rows = &v["blocks"][0]["blocks"][0]["rows"];
    let row = rows.as_array().unwrap().iter().find(|r| r["title"] == host);
    let row = row.unwrap_or_else(|| panic!("no {host} row: {v}"));
    let all = row["swipe"].as_array().unwrap().iter();
    all.map(|b| (b["action"].as_str().unwrap().to_string(), b["values"].clone()))
        .collect()
}

#[test]
fn the_hosts_sheet_lists_main_first_then_a_to_z_with_state_and_lanes() {
    let (c, wt) = fleet("hosts-screen");
    let sh = FakeShed::attached(&c);
    let r = runner(&wt);
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    s.refresh(NOW);
    let (status, v) = get(&s, "/screens/hosts");
    assert_eq!(status, 200, "{v}");
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/page/hosts.json");
    let got = serde_json::to_string_pretty(&v).unwrap() + "\n";
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&file, &got).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap_or_default(),
        got,
        "`BLESS=1` rewrites it"
    );
    let how = serde_json::to_string(&get(&s, "/screens/proxy-token").1).unwrap();
    for step in [
        "claude setup-token",
        "pbpaste > /nonexistent/drover/proxy",
        "read -rs t",
        "chmod 600 /nonexistent/drover/proxy",
    ] {
        assert!(how.contains(step), "{step}: {how}");
    }
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    assert!(page.contains(r#""label":"Hosts""#), "[⚙] lists Hosts: {page}");
}

#[test]
fn parity_75() {
    let (c, wt) = fleet("hosts-toggle");
    let sh = FakeShed::attached(&c);
    let r = runner(&wt);
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    s.refresh(NOW);
    let off = json!({"host": "cloud1", "to": "off"});
    assert_eq!(post(&s, "", "host", &off).0, 401, "only a paired device acts");
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    assert_eq!(
        swipe(&get(&s, "/screens/hosts").1, "cloud1"),
        [("host".into(), off.clone())]
    );
    let (status, v) = post(&s, &ck, "host", &off);
    assert_eq!((status, &v["toast"]), (200, &json!("cloud1 off")), "{v}");
    let on = json!({"host": "cloud1", "to": "on"});
    assert_eq!(swipe(&v["ops"][0]["screen"], "cloud1"), [("host".into(), on.clone())]);
    let (status, v) = post(&s, &ck, "host", &on);
    assert_eq!((status, &v["toast"]), (200, &json!("cloud1 on")), "{v}");
    let kinds: Vec<String> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind.starts_with("host-"))
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["host-off", "host-on"]);
    assert_eq!(swipe(&v["ops"][0]["screen"], "mini")[0].0, "reap");
    let (status, v) = post(&s, &ck, "reap", &json!({}));
    assert_eq!(
        (status, &v["toast"]),
        (200, &json!("reaped 2 leftover process(es)")),
        "{v}"
    );
    assert!(r.called("kill -TERM 101 102"), "{:?}", r.calls.borrow());
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "lane-reaped"));
}
