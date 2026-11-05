//! #477: a conductor start that times out (or finds its name taken) but whose agent is up is adopted, not failed;
//! deliveries to one thread wait for its start; a failure row shows herdr's message, not its JSON.
mod common;
use common::*;
use herdsman::serve::Server;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const TIMEOUT: &str = r#"{"error":{"code":"timeout","message":"timed out waiting for agent startup"}}"#;
const TAKEN: &str = r#"{"error":{"code":"agent_name_taken","message":"agent name conductor-t1 is already used"}}"#;

fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    c
}

/// The thread's conductor folder, as herdr reports an agent's cwd.
fn folder(c: &herdsman::Ctx) -> String {
    c.state.with_file_name("conductor").join("t1").display().to_string()
}

/// herdr with `list` as its agents, ready to start a conductor in workspace w9 or type into one.
fn herdr(list: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", list)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr workspace close", "")
        .reply("herdr pane run", "")
        .reply("herdr pane send-keys", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
}

/// Post `text` to t1 from a paired phone (its delivery runs before `respond` returns); the event log after.
fn say(c: &herdsman::Ctx, r: &Fake, text: &str) -> String {
    let s = Server::new(c, r, Some(ADDR.into()));
    let cookie = pair(&s, c, "Mozilla/5.0 (iPhone)", T);
    let body = format!("op=say&t=t1&text={text}");
    let req = format!(
        "POST /thread HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, T)).into_owned();
    assert!(res.ends_with("\r\n\r\nsent\n"), "{res}");
    herdsman::events::jsonl(&c.state).unwrap()
}

fn starts(c: &herdsman::Ctx) -> Vec<herdsman::events::Event> {
    let all = herdsman::events::read(&c.state).unwrap();
    all.into_iter().filter(|e| e.kind == "conductor-start").collect()
}

#[test]
fn a_start_that_times_out_with_its_agent_up_is_adopted_and_its_workspace_kept() {
    let c = setup("adopt-timeout");
    let r =
        herdr(&agents(&[("conductor-t1", "working", &folder(&c), "w9:p1")])).once_err("herdr agent start", 1, TIMEOUT);
    let log = say(&c, &r, "hello");
    assert!(!log.contains("not delivered"), "{log}");
    assert!(!r.called("herdr workspace close"), "{:?}", r.calls.borrow());
    let s = &starts(&c)[0];
    assert_eq!(
        (s.str("pane"), s.str("workspace"), &s.data["adopted"]),
        (Some("w9:p1"), Some("w9"), &serde_json::json!(true))
    );
}

#[test]
fn a_name_taken_by_the_threads_own_live_agent_is_adopted_and_the_line_typed_in() {
    let c = setup("adopt-taken");
    let r = herdr(&agents(&[("conductor-t1", "idle", &folder(&c), "w16:p1")])).once_err("herdr agent start", 1, TAKEN);
    let log = say(&c, &r, "hello");
    assert!(!log.contains("not delivered"), "{log}");
    assert!(
        r.called("herdr agent prompt conductor-t1 [t1] owner: hello"),
        "{:?}",
        r.calls.borrow()
    );
    let s = &starts(&c)[0];
    assert_eq!(
        (s.str("pane"), s.str("workspace"), &s.data["adopted"]),
        (Some("w16:p1"), Some("w16"), &serde_json::json!(true))
    );
}

#[test]
fn a_name_taken_by_an_agent_in_another_folder_is_still_not_delivered() {
    let c = setup("adopt-elsewhere");
    let r =
        herdr(&agents(&[("conductor-t1", "idle", "/elsewhere/t1", "w16:p1")])).once_err("herdr agent start", 1, TAKEN);
    let log = say(&c, &r, "hello");
    assert!(log.contains("✗ not delivered: "), "{log}");
    assert!(r.called("herdr workspace close w9"), "{:?}", r.calls.borrow());
    assert!(starts(&c).is_empty());
}

#[test]
fn a_failed_start_shows_herdrs_message_not_its_json() {
    let c = setup("adopt-message");
    let r = herdr(NO_AGENTS).once_err("herdr agent start", 1, TIMEOUT);
    let log = say(&c, &r, "hello");
    assert!(
        log.contains(r#""text":"✗ not delivered: herdr agent start failed: timed out waiting for agent startup""#),
        "{log}"
    );
    assert!(r.called("herdr workspace close w9"), "{:?}", r.calls.borrow());
}

/// herdr whose `agent start` holds until the test's go (telling it it began), its agent listed only once started;
/// each `agent list` while the start is held is reported.
struct Slow {
    f: Fake,
    up: AtomicBool,
    list: String,
    began: Mutex<Sender<()>>,
    listed: Mutex<Sender<()>>,
    go: Mutex<Receiver<()>>,
}
impl herdsman::runner::Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        let mut out = self.f.run(prog, args)?;
        let is = |a: &str, b: &str| prog == "herdr" && args.starts_with(&[a.into(), b.into()]);
        if is("agent", "list") {
            if self.up.load(SeqCst) {
                out.stdout = self.list.clone();
            } else {
                drop(self.listed.lock().unwrap().send(()));
            }
        }
        if is("agent", "start") {
            self.began.lock().unwrap().send(()).unwrap();
            self.go.lock().unwrap().recv().unwrap();
            self.up.store(true, SeqCst);
        }
        Ok(out)
    }
}

#[test]
fn two_messages_while_a_start_is_in_flight_start_one_conductor_and_reach_it_in_order() {
    let c = setup("adopt-serial");
    let (began, began_rx) = channel();
    let (listed, listed_rx) = channel();
    let (go, go_rx) = channel();
    let r = Slow {
        f: herdr(NO_AGENTS),
        up: AtomicBool::new(false),
        list: agents(&[("conductor-t1", "idle", &folder(&c), "w9:p1")]),
        began: Mutex::new(began),
        listed: Mutex::new(listed),
        go: Mutex::new(go_rx),
    };
    std::thread::scope(|s| {
        let a = s.spawn(|| run(&c, &r, &["thread", "say", "t1", "one"]));
        began_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        while listed_rx.try_recv().is_ok() {}
        let b = s.spawn(|| run(&c, &r, &["thread", "say", "t1", "two"]));
        // Unserialized, the second delivery reads herdr's agents at once (none yet) and starts a second conductor.
        let raced = listed_rx.recv_timeout(Duration::from_millis(500)).is_ok();
        go.send(()).unwrap();
        go.send(()).unwrap();
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        assert_eq!((a.0, b.0), (0, 0), "{a:?} {b:?}");
        assert!(!raced, "the second delivery did not wait: {:?}", r.f.calls.borrow());
    });
    assert_eq!(r.f.count("herdr agent start"), 1, "{:?}", r.f.calls.borrow());
    let dir = c.state.with_file_name("conductor").join("t1");
    assert_eq!(std::fs::read_to_string(dir.join(".thread.md")).unwrap(), "owner: one\n");
    let typed = r.f.index("herdr agent prompt conductor-t1 [t1] owner: two");
    assert!(typed > r.f.index("herdr agent start"), "{:?}", r.f.calls.borrow());
}
