//! #401: a fleet without `[fleet.<n>.role.conductor]` runs its conductor on its impl role (profile `lane-conductor`),
//! and the [config] sheet's Conductor choices write that table in place (backup first, refused unless doctor passes).
mod common;
use common::*;
use herdsman::events::{read, Event};
use herdsman::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

fn get(s: &Server, path: &str) -> String {
    String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T)).into_owned()
}

/// `CONFIG` (impl: hermes, opus; no conductor role) with rev on sonnet: the fleet's roles use opus and sonnet.
fn setup(name: &str) -> herdsman::Ctx {
    let text = CONFIG.replace(
        "profile = \"fleet-rev\"\n",
        "profile = \"fleet-rev\"\nmodel = \"sonnet\"\n",
    );
    ctx(&tmpdir(name), &text)
}

/// herdr ready to start a conductor in a new workspace; `which hermes` finds it (doctor's harness row).
fn host() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr pane", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
        .reply("which hermes", "/usr/local/bin/hermes\n")
}

fn starts(c: &herdsman::Ctx) -> Vec<Event> {
    let all = read(&c.state).unwrap().into_iter();
    all.filter(|e| e.kind == "conductor-start").collect()
}

fn start_line(r: &Fake) -> String {
    let calls = r.calls.borrow();
    let start = calls.iter().find(|c| c.starts_with("herdr agent start conductor-"));
    start.cloned().unwrap_or_default()
}

#[test]
fn a_fleet_without_a_conductor_role_starts_one_on_its_impl_role_from_the_board() {
    let c = setup("cdef-start");
    // `profiles sync` always writes the default conductor's profile.
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let profile = c.config.with_file_name("claude").join("lane-conductor");
    assert!(profile.join("settings.json").exists());
    let r = host();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Grill+%2312", &cookie);
    assert!(res.ends_with("\r\n\r\nt1 started\n"), "{res}");
    let st = &starts(&c)[0];
    assert_eq!((st.str("harness"), st.str("model")), (Some("hermes"), Some("opus")));
    assert!(start_line(&r).contains("-p lane-conductor"), "{}", start_line(&r));
    // The conductor answers with `thread reply`; `thread show` reads it back.
    let (code, out) = run(&c, &Fake::default(), &["thread", "reply", "t1", "Scope first."]);
    assert_eq!(code, 0, "{out}");
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert_eq!(
        show,
        "t1 sendmeter: Grill #12\nowner: Grill #12\nconductor: Scope first.\n"
    );
}

#[test]
fn a_conductor_choice_in_config_writes_its_table_after_a_backup_and_the_next_start_uses_it() {
    let c = setup("cdef-set");
    let orig = std::fs::read_to_string(&c.config).unwrap();
    let r = host();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let sheet = get(&s, "/frag?id=config&fleet=sendmeter");
    assert!(
        sheet.contains("<h3>Conductor<span>default: impl</span></h3>"),
        "{sheet}"
    );
    assert!(sheet.contains(
        "<b class=m>model</b><button name=model value=\"opus\" aria-pressed=true>opus</button>\
         <button name=model value=\"sonnet\" aria-pressed=false>sonnet</button></form>"
    ));
    let i = sheet.find("name=mtime value=").unwrap() + 17;
    let mt: String = sheet[i..].chars().take_while(char::is_ascii_digit).collect();
    let tap = format!("op=cond&fleet=sendmeter&model=sonnet&mtime={mt}");
    // Only a paired device saves.
    let res = req(&s, "/", &tap, "");
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), orig);
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/", &tap, &cookie);
    assert!(
        res.contains("Location: /?msg=sendmeter%20conductor%3A%20model%20sonnet"),
        "{res}"
    );
    let want = format!(
        "{orig}\n[fleet.sendmeter.role.conductor]\nharness = \"hermes\"\nmodel = \"sonnet\"\nprofile = \"lane-conductor\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap(),
        want,
        "every other byte kept"
    );
    assert_eq!(
        std::fs::read_to_string(c.config.with_extension("toml.bak")).unwrap(),
        orig
    );
    let set = read(&c.state).unwrap().into_iter().rfind(|e| e.kind == "set").unwrap();
    assert_eq!(
        (set.str("role"), set.str("key"), set.str("new")),
        (Some("conductor"), Some("model"), Some("sonnet"))
    );
    let sheet = get(&s, "/frag?id=config&fleet=sendmeter");
    assert!(sheet.contains("<h3>Conductor<span>set</span></h3>"), "{sheet}");
    // The next conductor start runs on the new model.
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Next", &cookie);
    assert!(res.ends_with("t1 started\n"), "{res}");
    assert_eq!(starts(&c)[0].str("model"), Some("sonnet"));
    assert!(start_line(&r).contains("-p lane-conductor"));
}

#[test]
fn a_conductor_choice_that_fails_doctor_is_refused_and_leaves_fleet_toml_alone() {
    let c = setup("cdef-doctor");
    let orig = std::fs::read(&c.config).unwrap();
    let r = host();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let sheet = get(&s, "/frag?id=config&fleet=sendmeter");
    let i = sheet.find("name=mtime value=").unwrap() + 17;
    let mt: String = sheet[i..].chars().take_while(char::is_ascii_digit).collect();
    // claude is not on this host's PATH: doctor gains `✗ harness — claude on PATH`.
    let res = req(
        &s,
        "/",
        &format!("op=cond&fleet=sendmeter&harness=claude&mtime={mt}"),
        &cookie,
    );
    assert!(
        res.contains("REFUSED%20DOCTOR") && res.contains("claude%20on%20PATH"),
        "{res}"
    );
    assert_eq!(std::fs::read(&c.config).unwrap(), orig, "byte-identical");
    assert!(!c.config.with_extension("toml.bak").exists());
    // A model none of the fleet's roles use is not a choice.
    let res = req(
        &s,
        "/",
        &format!("op=cond&fleet=sendmeter&model=haiku&mtime={mt}"),
        &cookie,
    );
    assert!(res.contains("REFUSED%20BAD%5FVALUE"), "{res}");
    assert_eq!(std::fs::read(&c.config).unwrap(), orig);
}
