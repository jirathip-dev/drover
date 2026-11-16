mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const RECIPE: &str =
    "key_env = \"FAKE_CLOUD_KEY\"\nstart = \"fakecloud start {name}\"\nstop = \"fakecloud stop {name}\"\n\
status = \"fakecloud status {name}\"\naddress = \"fakecloud address {name}\"\ndestroy = \"fakecloud destroy {name}\"\n\
[rate_usd_hour]\nsmall = 1.0\n";

fn fleet(name: &str) -> drover::Ctx {
    let hosts =
        "[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.cloud1]\nmachine = \"cloud1\"\ncloud = \"fake\"\nsize = \"small\"\n\
budget_usd_month = 10\nmax_hours_day = 20\ntags = [\"docker\"]\n";
    let c = ctx(&tmpdir(name), &format!("{CONFIG}\n{hosts}"));
    std::fs::create_dir_all(c.config.with_file_name("recipes")).unwrap();
    std::fs::write(c.config.with_file_name("recipes/fake.toml"), RECIPE).unwrap();
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let seen = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "cloud1": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW - 9000, "up": false},
    });
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    let rows: String = [(NOW - 3000, 0.5, 2), (NOW - 1200, 1.25, 3), (NOW - 60, 0.75, 1)]
        .iter()
        .map(|(ts, load, lanes)| {
            json!({"ts": ts, "host": "mini", "load_cpu": load, "pressure": 0, "swap_out": 10.0,
                "mem_free_gb": 9.5, "disk_free_gb": 120.0, "lanes": lanes})
            .to_string()
                + "\n"
        })
        .collect();
    std::fs::write(st.join("host-history.jsonl"), rows).unwrap();
    std::fs::create_dir_all(c.worktrees.join("stray-repo")).unwrap();
    c
}

fn runner() -> Fake {
    Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 2.0 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            "df -k /",
            "Filesystem 1024-blocks Used Available\n/dev/disk3 488245288 400000000 44040192\n",
        )
}

fn get(s: &Server, path: &str, extra: &str) -> (u16, Value) {
    let r = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn golden(name: &str, v: &Value) {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/page/{name}.json"));
    let got = serde_json::to_string_pretty(v).unwrap() + "\n";
    if std::env::var_os("BLESS").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &got).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap_or_default(),
        got,
        "{name}: `BLESS=1` rewrites it"
    );
}

#[test]
fn the_hosts_screen_has_a_card_per_host_main_first_and_the_fleets_disk_services_and_providers() {
    let c = fleet("hosts-screen");
    let sh = FakeShed::attached(&c);
    let r = runner();
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let (status, v) = get(&s, "/screens/hosts", "");
    assert_eq!(status, 200, "{v}");
    golden("hosts", &v);
    let (status, chart) = get(&s, "/screens/hosts/mini", "");
    assert_eq!(status, 200, "{chart}");
    golden("hosts-chart", &chart);
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    assert!(
        page.contains(r#""screen":"/screens/hosts","subtitle""#),
        "[⚙] lists Hosts: {page}"
    );
}

#[test]
fn a_paired_phone_turns_a_host_off_and_back_on_from_the_hosts_screen() {
    let c = fleet("hosts-toggle");
    let sh = FakeShed::attached(&c);
    let r = runner();
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    let post = |to: &str| {
        let body = json!({"screen": "hosts", "block": format!("host-cloud1-{to}"), "action": "host",
            "values": {"host": "cloud1", "to": to}})
        .to_string();
        let r = format!(
            "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{ck}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
        let (head, body) = res.split_once("\r\n\r\n").unwrap();
        (
            head[9..12].parse::<u16>().unwrap(),
            serde_json::from_str::<Value>(body).unwrap(),
        )
    };
    let button = |v: &Value| {
        let card = v["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == "host-cloud1")
            .cloned();
        card.unwrap_or_else(|| panic!("no cloud1 card: {v}"))["buttons"][0]["values"]["to"].clone()
    };
    assert_eq!(button(&get(&s, "/screens/hosts", "").1), "off");
    let (status, v) = post("off");
    assert_eq!((status, &v["toast"]), (200, &json!("cloud1 off")), "{v}");
    assert_eq!(button(&get(&s, "/screens/hosts", "").1), "on");
    let (status, v) = post("on");
    assert_eq!((status, &v["toast"]), (200, &json!("cloud1 on")), "{v}");
    let kinds: Vec<String> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind.starts_with("host-"))
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["host-off", "host-on"]);
    let unpaired = format!("POST /action HTTP/1.1\r\nHost: {ADDR}\r\nContent-Length: 2\r\n\r\n{{}}");
    assert!(String::from_utf8_lossy(&s.respond(&unpaired, NOW)).starts_with("HTTP/1.1 401"));
}
