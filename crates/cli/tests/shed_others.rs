mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const IPHONE: &str = "Mozilla/5.0 (iPhone) Safari/604.1";

fn board(name: &str, mini: &FakeShed) -> drover::Ctx {
    let hosts = "[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n";
    let c = ctx(&tmpdir(name), &mini.config(&format!("{CONFIG}\n{hosts}")));
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "dsh"});
    append(&c.state, &Event::new(T - 500, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let conductor = json!({"agent": "conductor-t1", "thread": "t1"});
    append(
        &c.state,
        &Event::new(T - 400, "sendmeter", None, "orchestrator-start", conductor),
    )
    .unwrap();
    mini.lane("sendmeter-impl-12", "working", "/w/issue-12");
    mini.add("conductor-t1", json!({}));
    c
}

fn chat(mini: &FakeShed, name: &str) {
    mini.add(name, json!({"app": "cooee", "thread": "c9"}));
    mini.set(name, |t| {
        t["acp"] = json!("agent-acp");
        t["reported"] = json!({"state": "idle"});
        t["events"] = json!([
            {"seq": 1, "event": "reply", "text": "an older reply", "stop_reason": "end_turn"},
            {"seq": 2, "event": "text", "text": "Sure."},
            {"seq": 3, "event": "reply", "text": "Sure.\nThe deploy finished.\n", "stop_reason": "end_turn"}
        ]);
    });
}

fn get(s: &Server, path: &str, cookie: &str, now: u64) -> (String, Value) {
    let res = String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"), now))
        .into_owned();
    let body = res[res.find("\r\n\r\n").unwrap() + 4..].to_string();
    (
        res.lines().next().unwrap_or_default().to_string(),
        serde_json::from_str(&body).unwrap_or_default(),
    )
}

#[test]
fn api_v1_shed_lists_each_hosts_other_sessions_not_the_lanes() {
    let (mini, air) = (FakeShed::new(), FakeShed::new());
    let c = board("shed-others", &mini);
    chat(&mini, "cooee-chat-1");
    air.add("notes", json!({}));
    air.set("notes", |t| t["reported"] = json!({"state": "working"}));
    air.screen("notes", "$ make\nbuilt ok\n\n");
    let f = Fake::default();
    let over = OverSsh {
        f: &f,
        hosts: vec![("air", &air)],
    };
    let r = drover::shed::With(&over, mini.shed());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, IPHONE, T);
    let (status, rows) = get(&s, "/api/v1/shed", &cookie, T);
    assert_eq!(status, "HTTP/1.1 200 OK", "{rows}");
    assert_eq!(
        rows,
        json!([
            {"name": "cooee-chat-1", "host": "mini", "runtime": "acp", "labels": {"app": "cooee", "thread": "c9"},
                "state": "idle", "last": "The deploy finished.", "age": "0s"},
            {"name": "notes", "host": "air", "runtime": "shed", "labels": {}, "state": "working", "last": "built ok",
                "age": "0s"}
        ])
    );
    assert!(f.called("ssh") && f.calls.borrow().iter().any(|l| l.contains("air -- 'shed' 'connect'")));
    let lists = |sh: &FakeShed| sh.methods().iter().filter(|m| *m == "ls").count();
    let (m, a) = (lists(&mini), lists(&air));
    let (_, again) = get(&s, "/api/v1/shed", &cookie, T + 5);
    assert_eq!(
        (lists(&mini), lists(&air)),
        (m, a),
        "a refresh within 10 s lists no shed again"
    );
    assert_eq!(again[0]["age"], "5s");
    air.set("notes", |t| t["reported"] = json!({"state": "blocked"}));
    let (_, later) = get(&s, "/api/v1/shed", &cookie, T + 130);
    assert_eq!(
        (lists(&mini), lists(&air)),
        (m + 1, a + 1),
        "one list per host per refresh"
    );
    assert_eq!(
        (&later[0]["age"], &later[1]["state"], &later[1]["age"]),
        (&json!("2m"), &json!("blocked"), &json!("0s"))
    );
}

#[test]
fn a_host_whose_shed_fails_is_one_error_entry_and_stays_up() {
    let mini = FakeShed::new();
    let c = board("shed-fails", &mini);
    chat(&mini, "cooee-chat-2");
    let f = Fake::default().once_err("ssh", 255, "ssh: connect to host air port 22: Operation timed out");
    let over = OverSsh { f: &f, hosts: vec![] };
    let r = drover::shed::With(&over, mini.shed());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, IPHONE, T);
    let (status, rows) = get(&s, "/api/v1/shed", &cookie, T);
    assert_eq!(status, "HTTP/1.1 200 OK");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0]["name"], "cooee-chat-2");
    assert_eq!(
        rows[1],
        json!({"host": "air", "error": "shed connect failed (255): ssh: connect to host air port 22: Operation timed out"})
    );
    assert!(
        drover::hosts::cached(&c).get("air").is_none_or(|h| h.up),
        "air is never marked offline"
    );
    let (_, lanes) = get(&s, "/api/v1/lanes", &cookie, T);
    assert_eq!(lanes["shed"][1]["error"], rows[1]["error"]);
}

#[test]
fn an_unpaired_request_gets_401() {
    let mini = FakeShed::new();
    let c = board("shed-unpaired", &mini);
    chat(&mini, "cooee-chat-3");
    let f = Fake::default();
    let r = mini.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (status, body) = get(&s, "/api/v1/shed", "", T);
    assert_eq!(status, "HTTP/1.1 401 Unauthorized");
    assert_eq!(body["code"], "UNPAIRED");
    assert!(!mini.methods().contains(&"ls".to_string()), "nothing is read for it");
    let (_, lanes) = get(&s, "/api/v1/lanes", "", T);
    assert_eq!(lanes["shed"], json!([]), "an unpaired Lanes tab has no shed rows");
}

#[test]
fn the_lanes_tab_ends_with_the_folded_section_and_fits_390px() {
    let mini = FakeShed::new();
    let c = board("shed-tab", &mini);
    chat(&mini, "cooee-chat-4");
    mini.add("scratch-shell", json!({}));
    mini.screen(
        "scratch-shell",
        "~ $ cargo build --release 2>&1 | tail -1 && echo a very long last line that ellipsises",
    );
    mini.set("scratch-shell", |t| t["reported"] = Value::Null);
    let f = Fake::default().once_err("ssh", 255, "air: unreachable");
    let r = mini.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, IPHONE, T);
    let lanes = api(&s, ADDR, &cookie, T, "lanes");
    let html = &board_js(&[(&lanes, "lanes.tab(d)")])[0];
    let at = html
        .find("<details class=qm><summary><span>Other in shed</span> (2)</summary>")
        .expect(html);
    let section = &html[at..];
    assert!(section.ends_with("</details>"), "the tab's last part");
    for want in [
        "cooee-chat-4 <span class=\"tag m\">mini</span> <span class=\"tag m\">acp</span>",
        "<p class=\"act nw el\">The deploy finished.</p>",
        "<small>app=cooee thread=c9</small>",
        "scratch-shell <span class=\"tag m\">mini</span> <span class=\"tag m\">shed</span>",
        "running <span class=m>0s</span>",
        "<p class=\"r nw el\">! shed air: shed connect failed (255): air: unreachable</p>",
    ] {
        assert!(section.contains(want), "{want}\n{section}");
    }
    assert!(!section.contains("<button") && !section.contains("<form") && !section.contains("data-agent"));
    if no_webkit() {
        return;
    }
    let res =
        String::from_utf8_lossy(&s.respond(&format!("GET /?all=1 HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T)).into_owned();
    let d = tmpdir("shed-png");
    let file = d.join("page.html");
    std::fs::write(
        &file,
        drawn(&res[res.find("<!doctype").unwrap()..], &s, ADDR, &cookie, T),
    )
    .unwrap();
    let dir = std::env::var("DROVER_SNAPSHOTS").map_or(d.clone(), std::path::PathBuf::from);
    let open = "const x = document.querySelector('#lanes details.qm'); x.open = true; x.scrollIntoView()";
    for (name, js) in [
        (
            "shed-folded",
            "document.querySelector('#lanes details.qm').scrollIntoView()",
        ),
        ("shed-open", open),
    ] {
        let png = dir.join(format!("{name}.png"));
        let o = webview()
            .args([
                file.to_str().unwrap(),
                "390",
                "lanes",
                "horse",
                png.to_str().unwrap(),
                js,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let w: Vec<u32> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && w.len() == 2,
            "{name}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(w[0] <= 390 && w[1] == 390, "{name}: scrollWidth {} at {}", w[0], w[1]);
        assert!(png.exists(), "{name}");
    }
}
