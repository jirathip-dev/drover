mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;

fn get(s: &Server, path: &str) -> Value {
    let r = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{path}: {res}");
    serde_json::from_str(body).unwrap()
}

fn post(s: &Server, auth: &str, screen: &str, action: &str, values: &Value) -> (u16, Value) {
    let body = json!({"screen": screen, "block": "", "action": action, "values": values}).to_string();
    let r = format!(
        "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
    fn go<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
        match v {
            Value::Object(m) if m.get("id").and_then(Value::as_str) == Some(id) => Some(v),
            Value::Object(m) => m.values().find_map(|x| go(x, id)),
            Value::Array(a) => a.iter().find_map(|x| go(x, id)),
            _ => None,
        }
    }
    go(v, id).unwrap_or_else(|| panic!("no {id} in {v}"))
}

fn plugin(c: &drover::Ctx) -> String {
    std::fs::write(c.config.with_file_name("cooee-token"), "tok-1378\n").unwrap();
    "Authorization: Bearer tok-1378\r\n".into()
}

#[test]
fn parity_77_the_hosts_sheet_shows_worktree_disk_and_removes_orphans_on_a_confirm() {
    let c = ctx(&tmpdir("parity-77"), CONFIG);
    std::fs::create_dir_all(c.worktrees.join("sendmeter/issue-12")).unwrap();
    std::fs::create_dir_all(c.worktrees.join("gone-repo/issue-3")).unwrap();
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(
        c.state.with_file_name("worktrees.du"),
        format!("32212254720 {}\n", NOW - 60),
    )
    .unwrap();
    let f = Fake::default();
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let v = get(&s, "/screens/hosts");
    let wt = find(&v, "disk-worktrees");
    assert_eq!(wt["value"], "30.0 / 25 GB", "{wt}");
    assert_eq!(wt["subtitle"]["tone"], "warn", "over worktree_budget_gb: {wt}");
    let orphans = find(&v, "disk-orphans");
    assert_eq!(orphans["value"], "1", "{orphans}");
    let swipe = &orphans["swipe"][0];
    assert_eq!((&swipe["action"], &swipe["values"]), (&json!("orphans"), &json!({})));
    assert_eq!(swipe["confirm"]["title"], "Remove 1 orphan folder?");

    let auth = plugin(&c);
    let (status, v) = post(&s, &auth, "hosts", "orphans", &json!({}));
    assert_eq!(
        (status, &v["toast"]),
        (200, &json!("removed 1 orphan folder(s)")),
        "{v}"
    );
    assert!(!c.worktrees.join("gone-repo").exists() && c.worktrees.join("sendmeter/issue-12").exists());
    let sheet = &v["ops"][0]["screen"];
    assert_eq!(find(sheet, "disk-orphans")["value"], "0");
    assert!(find(sheet, "disk-orphans").get("swipe").is_none(), "{sheet}");

    std::fs::remove_file(c.state.with_file_name("worktrees.du")).unwrap();
    let v = get(&s, "/screens/hosts");
    assert_eq!(find(&v, "disk-worktrees")["value"], "not measured yet / 25 GB");
    let (status, v) = post(&s, "", "hosts", "orphans", &json!({}));
    assert_eq!(status, 401, "only a paired device or the plugin: {v}");
}

#[test]
fn parity_83_events_group_by_time_and_important_hides_the_noise() {
    let c = ctx(&tmpdir("parity-83"), CONFIG);
    let ev = |ago: u64, n: u64, kind: &str, d: Value| {
        append(&c.state, &Event::new(NOW - ago, "sendmeter", Some(n), kind, d)).unwrap();
    };
    ev(136_200, 1, "merged", json!({"pr": "1", "into": "staging"}));
    ev(60_000, 2, "verdict", json!({"verdict": "PASS", "round": 1}));
    ev(7_200, 3, "verdict", json!({"verdict": "FAIL", "round": 1}));
    ev(600, 4, "merged", json!({"pr": "4", "into": "staging"}));
    ev(60, 5, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-5"}));
    let f = Fake::default();
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let groups = |v: &Value| -> Vec<(String, Vec<String>)> {
        let blocks = v["blocks"].as_array().unwrap().iter();
        blocks
            .filter(|b| b["id"].as_str().is_some_and(|i| i.starts_with("ev-")))
            .map(|g| {
                let rows = g["blocks"][0]["rows"].as_array().unwrap().iter();
                let titles = rows.map(|r| r["title"].as_str().unwrap().to_string()).collect();
                (g["header"].as_str().unwrap().to_string(), titles)
            })
            .collect()
    };
    let v = get(&s, "/screens/events");
    let g = groups(&v);
    let heads: Vec<(&str, usize)> = g.iter().map(|(h, r)| (h.as_str(), r.len())).collect();
    assert_eq!(heads, [("last hour", 2), ("today", 1), ("yesterday", 1)], "{v}");
    assert!(g[0].1[0].starts_with("impl started"), "{g:?}");
    let chips = find(&v, "kind");
    assert_eq!(
        chips["options"].as_array().unwrap().last().unwrap(),
        &json!({"value": "imp", "label": "important"})
    );
    assert_eq!(chips["selected"], json!(["all"]));

    let v = get(&s, "/screens/events?kind=all,imp");
    let g = groups(&v);
    let heads: Vec<(&str, usize)> = g.iter().map(|(h, r)| (h.as_str(), r.len())).collect();
    assert_eq!(heads, [("last hour", 1), ("today", 1), ("yesterday", 1)], "{v}");
    assert!(g.iter().all(|(_, r)| r.iter().all(|t| !t.contains("started"))), "{g:?}");
    assert_eq!(find(&v, "kind")["selected"], json!(["all", "imp"]));
    assert_eq!(v["stream"], "/screens/events/stream?kind=all,imp");

    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    let (status, r) = post(&s, &ck, "events", "filter", &json!({"kind": ["merge", "imp"]}));
    assert_eq!(status, 200, "{r}");
    let g = groups(&r["ops"][0]["screen"]);
    assert_eq!(g.iter().map(|x| x.1.len()).sum::<usize>(), 1, "{g:?}");
    assert!(g[0].1[0].starts_with("merged into staging · PR #4"), "{g:?}");

    let v = get(&s, &format!("/screens/events?before={}", NOW - 50_000));
    let heads: Vec<String> = groups(&v).into_iter().map(|x| x.0).collect();
    assert_eq!(heads, ["yesterday", "earlier"], "{v}");
}

#[test]
fn parity_84_the_drover_menu_opens_hosts_and_the_notifications_sheet() {
    let cfg = format!(
        "{CONFIG}\n[[notify]]\ncmd = \"notify-send drover\"\non = [\"merged\", \"needs-human\"]\n\n\
[[notify]]\ntype = \"webpush\"\nsub = \"mailto:owner@example.com\"\n\n\
[[fleet.sendmeter.notify]]\ncmd = \"curl -s https://hooks.example.com/drover\"\nformat = \"json\"\n"
    );
    let c = ctx(&tmpdir("parity-84"), &cfg);
    let f = Fake::default();
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let fleets = get(&s, "/screens/fleets");
    assert_eq!(find(&fleets, "menu-hosts")["tap"], json!({"sheet": "/screens/hosts"}));
    let row = find(&fleets, "menu-notify");
    assert_eq!(row["tap"], json!({"sheet": "/screens/notify"}));
    assert_eq!(row["subtitle"], "3 notifiers · Telegram off");

    let v = get(&s, "/screens/notify");
    assert_eq!((&v["type"], &v["title"]), (&json!("sheet"), &json!("Notifications")));
    let g0 = find(&v, "nf-global-0");
    assert_eq!(
        (&g0["title"], &g0["subtitle"]),
        (&json!("notify-send drover"), &json!("command · merged, needs-human"))
    );
    let g1 = find(&v, "nf-global-1");
    assert_eq!(
        (&g1["title"], &g1["subtitle"]),
        (&json!("mailto:owner@example.com"), &json!("webpush · push events"))
    );
    assert_eq!(
        find(&v, "nf-sendmeter")["header"],
        "sendmeter (instead of the global ones)"
    );
    let own = find(&v, "nf-sendmeter-0");
    assert_eq!(own["subtitle"], "command · json · pauses, reverts and stalls only");
    assert_eq!(own["title"], "curl -s https://hooks.example.com/drover");
    let tg = find(&v, "telegram");
    assert_eq!(tg["badge"], json!({"text": "off", "tone": "mute"}));

    let c = ctx(
        &tmpdir("parity-84-tg"),
        &format!("{CONFIG}\n[telegram]\ntoken_file = \"x\"\n"),
    );
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let v = get(&s, "/screens/notify");
    assert_eq!(find(&v, "nf-global-none")["title"], "no notifiers");
    assert_eq!(find(&v, "telegram")["subtitle"]["tone"], "warn", "{v}");
}

#[test]
fn parity_81_every_configured_role_gets_a_model_and_an_effort_picker_and_saving_sets_it() {
    let cfg = CONFIG.replacen(
        "[limits]",
        "[fleet.sendmeter.role.verify]\nharness = \"claude\"\neffort = \"low\"\n\n[limits]",
        1,
    );
    let c = ctx(&tmpdir("parity-81"), &format!("models = [\"opus\", \"sonnet\"]\n{cfg}"));
    let f = Fake::default();
    let s = Server::new(&c, &f, Some(ADDR.into()));
    let v = get(&s, "/screens/fleets");
    let fields = find(&v, "f-sendmeter")["fields"].as_array().unwrap().clone();
    let names: Vec<&str> = fields.iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "sendmeter.drive",
            "sendmeter.integration",
            "sendmeter.impl",
            "sendmeter.impl.effort",
            "sendmeter.rev",
            "sendmeter.rev.effort",
            "sendmeter.verify",
            "sendmeter.verify.effort"
        ]
    );
    let efforts: Vec<&str> = drover::providers::get("hermes").efforts().to_vec();
    let pick = |n: &str| fields.iter().find(|x| x["name"] == n).unwrap().clone();
    assert_eq!(pick("sendmeter.rev.effort")["options"], json!(efforts));
    assert_eq!(pick("sendmeter.rev.effort")["label"], "Review effort");
    assert_eq!(pick("sendmeter.verify.effort")["value"], "low");
    assert_eq!(pick("sendmeter.verify")["options"], json!(["opus", "sonnet"]));

    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    let values =
        json!({"sendmeter.rev.effort": "high", "sendmeter.verify": "sonnet", "sendmeter.verify.effort": "low"});
    let (status, r) = post(&s, &ck, "fleets", "save-fleets", &values);
    assert_eq!(status, 200, "{r}");
    assert_eq!(r["toast"], "sendmeter rev: effort high; sendmeter verify: model sonnet");
    let kept = drover::config::Config::load(&c.config).unwrap();
    let role = |r: &str| kept.fleet["sendmeter"].role[r].clone();
    assert_eq!(
        role("rev").effort.as_deref(),
        Some("high"),
        "the next review lane runs with it"
    );
    assert_eq!(role("verify").model.as_deref(), Some("sonnet"));
    let set = read(&c.state).unwrap().iter().filter(|e| e.kind == "set").count();
    assert_eq!(set, 2);
    let (status, r) = post(
        &s,
        &ck,
        "fleets",
        "save-fleets",
        &json!({"sendmeter.rev.effort": "high"}),
    );
    assert_eq!((status, &r["toast"]), (200, &json!("nothing changed")), "{r}");
    let (status, r) = post(
        &s,
        &ck,
        "fleets",
        "save-fleets",
        &json!({"sendmeter.rev.effort": "turbo\""}),
    );
    assert_eq!(status, 409, "a quote is refused by `set`: {r}");
}
