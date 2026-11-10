//! #412: the optional per-issue design stage: `queue add --design`, drive's design lane, the hand-off to the owner, and
//! approve / revise / reject from the board card or the conductor thread.
mod common;
use common::*;
use herdsman::events::{fold, read, Event};
use herdsman::runner::{Out, Real, Runner};
use herdsman::serve::Server;
use std::path::Path;
use std::sync::Mutex;

/// `HERDSMAN_THREAD` is process-wide: the tests here run one at a time.
static ONE: Mutex<()> = Mutex::new(());
const ADDR: &str = "127.0.0.1:7777";
const DESIGN: &str = "\n[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";
const HEAD: &str = "dddddddddddddddddddddddddddddddddddddddd";

fn setup(name: &str, extra: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), &format!("{CONFIG}{DESIGN}{extra}"));
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    c
}

fn wt(c: &herdsman::Ctx, n: u64) -> String {
    c.worktrees.join(format!("sendmeter/issue-{n}")).display().to_string()
}

/// Spawns succeed, the host is calm; #12's design worktree is at the pushed commit `head`, touching two PNGs.
fn fake(c: &herdsman::Ctx, head: &str) -> Fake {
    let w = wt(c, 12);
    Fake::default()
        .reply("herdr agent list", &agents(&[("conductor-t1", "idle", "/x", "w9:c1")]))
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply(&format!("git -C {w} log -1"), &format!("{head}\n"))
        .reply(&format!("git -C {w} rev-list"), "0\n")
        .reply(
            &format!("git -C {w} diff --name-only"),
            "docs/design/meter/index.html\ndocs/design/meter/1440.png\ndocs/design/meter/390.png\n",
        )
        .reply("git -C", "")
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:c1"}}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent", "{}")
}

fn drive(c: &herdsman::Ctx, r: &dyn Runner) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn state(c: &herdsman::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn spawned(c: &herdsman::Ctx, role: &str) -> usize {
    kinds(c, "spawn").iter().filter(|e| e.str("role") == Some(role)).count()
}

#[test]
fn a_marked_issue_gets_design_until_approved_and_an_unmarked_one_goes_straight_to_impl() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = setup("design-flow", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "13"]).0, 0);
    let r = fake(&c, HEAD);
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-design-12") && !r.called("herdr agent start sendmeter-impl-12"));
    assert!(
        r.called("herdr agent start sendmeter-impl-13"),
        "unmarked: impl at once"
    );
    assert!(r.called("herdr worktree create --cwd /tmp/does-not-matter/sendmeter --branch issue-12"));
    assert_eq!((state(&c, 12).as_str(), state(&c, 13).as_str()), ("design", "impl"));
    // The lane's turn ended with a pushed commit: the owner gets it, with both PNGs.
    let out = drive(&c, &r);
    assert!(
        out.contains(&format!("sendmeter #12 design-review: {:.7}", HEAD)),
        "{out}"
    );
    assert_eq!(state(&c, 12), "design-review");
    let ask = &kinds(&c, "needs-decision")[0];
    let files: Vec<&str> = ask.data["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["docs/design/meter/1440.png", "docs/design/meter/390.png"]);
    let (_, st) = run(&c, &r, &["status", "sendmeter"]);
    assert!(st.contains("design-review"), "{st}");
    let (_, w) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(w.contains("herdsman answer sendmeter q1 approve|revise|reject"), "{w}");
    drive(&c, &r);
    assert_eq!(spawned(&c, "impl"), 1, "never impl before the approval");
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "approve"]).0, 0);
    assert_eq!(kinds(&c, "design-approved").len(), 1);
    assert!(r.called("herdr pane close w9:p1"), "the design pane goes");
    // #778: approved on a busy host: impl's start stays pending with the reason, then starts once load drops.
    let busy = fake(&c, HEAD).once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    drive(&c, &busy);
    let p = herdsman::pending::of(&read(&c.state).unwrap(), "sendmeter", 12).expect("pending");
    assert_eq!((p.action.as_str(), p.reason.as_deref()), ("approve", Some("HOST_BUSY")));
    assert_eq!(spawned(&c, "impl"), 1, "only #13's");
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert_eq!((state(&c, 12).as_str(), spawned(&c, "design")), ("impl", 1));
    assert_eq!(herdsman::pending::of(&read(&c.state).unwrap(), "sendmeter", 12), None);
}

/// A paired device's `POST path` with `body`.
fn post(s: &Server, path: &str, body: &str, cookie: &str, now: u64) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

/// The needs cards and the [chat] tab as the board draws them (#269) for the device.
fn needs_and_chat(s: &Server, cookie: &str, now: u64) -> (String, String) {
    let [needs, _, chat] = parts(s, ADDR, cookie, now);
    (needs, chat)
}

#[test]
fn the_board_card_and_the_thread_ask_are_one_answer_first_tap_wins() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = setup(
        "design-surfaces",
        "\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n",
    );
    let r = fake(&c, HEAD);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    assert!(post(&s, "/thread", "op=new&repo=sendmeter&text=Design+first", &cookie, c.now).contains("t1"));
    // The conductor queues both issues `--design` from its thread.
    std::env::set_var("HERDSMAN_THREAD", "t1");
    let q = run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "12", "13", "--design"],
    );
    std::env::remove_var("HERDSMAN_THREAD");
    assert_eq!(q.0, 0, "{}", q.1);
    let png = Path::new(&wt(&c, 12)).join("docs/design/meter");
    std::fs::create_dir_all(&png).unwrap();
    std::fs::write(png.join("1440.png"), b"\x89PNG wide").unwrap();
    drive(&c, &r);
    drive(&c, &r);
    // #12 hands off (its worktree has the commit); #13's lane is still at it.
    let ask = &kinds(&c, "needs-decision")[0];
    assert_eq!((ask.issue, ask.str("thread")), (Some(12), Some("t1")));
    let (needs, chat) = needs_and_chat(&s, &cookie, c.now + 5);
    assert!(
        needs.contains("<div class=ask>") && needs.contains("<img src=\"design/sendmeter/12/0\""),
        "{needs}"
    );
    assert!(chat.contains("data-id=\"q1\""), "{chat}");
    let res = String::from_utf8_lossy(&s.respond(
        &format!("GET /design/sendmeter/12/0 HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"),
        c.now,
    ))
    .into_owned();
    assert!(
        res.starts_with("HTTP/1.1 200") && res.ends_with("\u{fffd}PNG wide"),
        "{res}"
    );
    // Approve on the board: the thread's rows fold; the thread's own tap is then refused.
    let i = needs.find("name=mtime value=").unwrap() + 17;
    let mt: String = needs[i..].chars().take_while(char::is_ascii_digit).collect();
    let res = post(
        &s,
        "/",
        &format!("op=answer&fleet=sendmeter&id=q1&choice=1&mtime={mt}"),
        &cookie,
        c.now + 10,
    );
    assert!(res.starts_with("HTTP/1.1 303"), "{res}");
    assert_eq!(kinds(&c, "design-approved")[0].str("by"), Some("owner"));
    let (needs, chat) = needs_and_chat(&s, &cookie, c.now + 15);
    assert!(
        !needs.contains("<div class=ask>") && !chat.contains("data-id=\"q1\""),
        "{needs}"
    );
    assert!(chat.contains("✓ 1. approve"), "{chat}");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "reject"]);
    assert!(code == 3 && out.contains("REFUSED ALREADY_ANSWERED"), "{out}");
    // The reverse, on #13: answered in the thread first, the board's answer is refused.
    std::fs::create_dir_all(wt(&c, 13)).unwrap();
    let w = wt(&c, 13);
    let r13 = Fake::default()
        .reply(&format!("git -C {w} log -1"), &format!("{}\n", "e".repeat(40)))
        .reply(&format!("git -C {w} diff --name-only"), "docs/design/meter/1440.png\n")
        .reply(&format!("git -C {w} rev-list"), "0\n");
    let r13 = Both(r13, &r);
    drive(&c, &r13);
    assert_eq!(kinds(&c, "needs-decision")[1].issue, Some(13));
    let res = post(
        &s,
        "/thread",
        "op=answer&fleet=sendmeter&id=q2&choice=1",
        &cookie,
        c.now + 20,
    );
    assert!(res.contains("answered q2"), "{res}");
    let res = post(
        &s,
        "/",
        &format!("op=answer&fleet=sendmeter&id=q2&choice=3&mtime={mt}"),
        &cookie,
        c.now + 30,
    );
    assert!(res.contains("REFUSED%20ALREADY%5FANSWERED"), "{res}");
    assert_eq!(kinds(&c, "design-approved").len(), 2);
    assert!(kinds(&c, "design-rejected").is_empty());
}

/// `a` first, then `b` for whatever `a` has no script for.
struct Both<'a>(Fake, &'a dyn Runner);
impl Runner for Both<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let o = self.0.run(prog, args)?;
        if o.stderr.starts_with("unscripted") {
            return self.1.run(prog, args);
        }
        Ok(o)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn revise_redoes_the_design_with_the_note_in_its_brief_five_times_in_a_row() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut c = setup("design-revise", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    for k in 1..=5 {
        // Each round's design lane pushes a new head.
        let r = fake(&c, &k.to_string().repeat(40));
        drive(&c, &r);
        drive(&c, &r);
        assert_eq!(state(&c, 12), "design-review", "round {k}");
        let note = format!("round {k}: make the meter blue");
        let (code, out) = run(&c, &r, &["answer", "sendmeter", &format!("q{k}"), &note]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(kinds(&c, "design-revise")[k - 1].str("note"), Some(note.as_str()));
        c.now += 1000;
        drive(&c, &r);
        let brief = std::fs::read_to_string(Path::new(&wt(&c, 12)).join(".brief.md")).unwrap();
        assert!(brief.contains("# ROLE: DESIGNER") && brief.contains(&note), "{brief}");
        assert_eq!((state(&c, 12).as_str(), spawned(&c, "design")), ("design", k + 1));
    }
    assert_eq!(spawned(&c, "impl"), 0);
}

/// `git` runs for real (in a temp repo); everything else is the fake.
struct RealGit(Fake);
impl Runner for RealGit {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" {
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn reject_pushes_one_commit_back_to_the_exact_pre_design_tree_and_needs_a_human() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let d = tmpdir("design-reject");
    let (origin, local) = (d.join("origin.git"), d.join("local"));
    git(&d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    git(&local, &["config", "user.email", "t@example.com"]);
    git(&local, &["config", "user.name", "t"]);
    std::fs::write(local.join("app.rs"), "fn main() {}\n").unwrap();
    git(&local, &["add", "."]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "origin", "HEAD:staging"]);
    let cfg = format!("{CONFIG}{DESIGN}").replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(&d, &cfg);
    // The lane's worktree on the issue branch (herdr makes it in a real spawn).
    let w = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(w.parent().unwrap()).unwrap();
    git(
        &local,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "issue-12",
            w.to_str().unwrap(),
            "origin/staging",
        ],
    );
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let r = RealGit(fake(&c, HEAD));
    drive(&c, &r);
    let pre = git(&w, &["rev-parse", "HEAD"]);
    assert_eq!(kinds(&c, "spawn")[0].str("sha"), Some(pre.as_str()));
    // The design lane's work: two commits, pushed.
    std::fs::create_dir_all(w.join("docs/design/meter")).unwrap();
    std::fs::write(w.join("docs/design/meter/1440.png"), "png").unwrap();
    std::fs::write(w.join("app.rs"), "fn main() { meter() }\n").unwrap();
    git(&w, &["add", "docs", "app.rs"]);
    git(&w, &["commit", "-qm", "design 1"]);
    std::fs::write(w.join("docs/design/meter/390.png"), "png").unwrap();
    git(&w, &["add", "docs"]);
    git(&w, &["commit", "-qm", "design 2"]);
    git(&w, &["push", "-q", "origin", "issue-12"]);
    let design = git(&w, &["rev-parse", "HEAD"]);
    drive(&c, &r);
    assert_eq!(state(&c, 12), "design-review");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "3"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state(&c, 12), "needs-human");
    let head = git(&w, &["rev-parse", "HEAD"]);
    assert_eq!(git(&w, &["diff", &pre, "HEAD"]), "", "the tree is the pre-design one");
    assert_eq!(
        git(&w, &["rev-parse", "HEAD^"]),
        design,
        "one commit on top of the design"
    );
    assert_eq!(git(&origin, &["rev-parse", "issue-12"]), head, "pushed");
    let rej = &kinds(&c, "design-rejected")[0];
    assert_eq!(
        (rej.str("pre"), rej.str("sha")),
        (Some(pre.as_str()), Some(head.as_str()))
    );
    drive(&c, &r);
    assert_eq!(spawned(&c, "impl"), 0);
}

#[test]
fn design_off_on_a_queued_issue_lets_impl_spawn_directly() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = setup("design-off", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let bad = run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "set", "12", "design", "maybe"],
    );
    assert_ne!(bad.0, 0, "design is `on` or `off`");
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "set", "12", "design", "off"]
        )
        .0,
        0
    );
    let r = fake(&c, HEAD);
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-impl-12") && !r.called("herdr agent start sendmeter-design-12"));
    assert_eq!(state(&c, 12), "impl");
}

#[test]
fn a_paused_fleet_auto_spawns_no_design_but_the_conductor_still_can() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = setup("design-paused", "");
    let toml = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &c.config,
        toml.replace("integration = \"staging\"", "integration = \"staging\"\npaused = true"),
    )
    .unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let r = fake(&c, HEAD);
    drive(&c, &r);
    assert!(!r.called("herdr agent start"), "{:?}", r.calls.borrow());
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "design"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called("herdr agent start sendmeter-design-12"));
    assert_eq!(state(&c, 12), "design");
}

const ARCHITECT: &str = "\n[fleet.sendmeter.role.architect]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";

/// The architect lane's pushed commit `head` touches `docs/architecture/meter/` (put before `fake`).
fn adr(c: &herdsman::Ctx, head: &str) -> Fake {
    let w = wt(c, 12);
    Fake::default().reply(
        &format!("git -C {w} diff --name-only {head} {head} -- docs/architecture"),
        "docs/architecture/meter/adr.md\ndocs/architecture/meter/statechart.md\n",
    )
}

fn brief(c: &herdsman::Ctx) -> String {
    std::fs::read_to_string(Path::new(&wt(c, 12)).join(".brief.md")).unwrap()
}

#[test]
fn design_and_architect_run_architect_then_design_then_impl_which_links_the_adr() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = setup("architect-flow", ARCHITECT);
    let q = ["queue", "sendmeter", "add", "12", "--design", "--architect"];
    assert_eq!(run(&c, &Fake::default(), &q).0, 0);
    let base = fake(&c, HEAD);
    let r = Both(adr(&c, HEAD), &base);
    drive(&c, &r);
    assert!(
        base.called("herdr agent start sendmeter-architect-12")
            && !base.called("herdr agent start sendmeter-design-12")
    );
    assert!(brief(&c).contains("# ROLE: ARCHITECT") && brief(&c).contains("docs/architecture/<slug>/"));
    assert_eq!(state(&c, 12), "design");
    drive(&c, &r);
    assert_eq!(state(&c, 12), "design-review");
    let ask = &kinds(&c, "needs-decision")[0];
    assert!(
        ask.str("question").unwrap().starts_with("Architecture (ADR) for #12"),
        "{ask:?}"
    );
    drive(&c, &r);
    assert_eq!(
        (spawned(&c, "design"), spawned(&c, "impl")),
        (0, 0),
        "nothing after the architect before approve"
    );
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "approve"]).0, 0);
    let ok = &kinds(&c, "design-approved")[0];
    assert_eq!(
        (ok.str("role"), ok.str("adr")),
        (Some("architect"), Some("docs/architecture/meter"))
    );
    drive(&c, &r);
    assert!(
        base.called("herdr agent start sendmeter-design-12"),
        "the design comes next"
    );
    assert_eq!((state(&c, 12).as_str(), spawned(&c, "impl")), ("design", 0));
    // The design lane pushes its own head on top of the approved ADR.
    let r = fake(&c, &"e".repeat(40));
    drive(&c, &r);
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q2", "approve"]).0, 0);
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert_eq!(state(&c, 12), "impl");
    let b = brief(&c);
    assert!(
        b.contains("the owner approved the ADR in `docs/architecture/meter/`"),
        "{b}"
    );
}

#[test]
fn architect_revise_reruns_it_with_the_note_and_reject_needs_a_human() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut c = setup("architect-revise", ARCHITECT);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "12", "--architect"]
        )
        .0,
        0
    );
    let base = fake(&c, HEAD);
    let r = Both(adr(&c, HEAD), &base);
    drive(&c, &r);
    drive(&c, &r);
    assert_eq!(state(&c, 12), "design-review");
    let note = "split the store from the sync";
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", note]).0, 0);
    c.now += 1000;
    let head = "e".repeat(40); // the revised ADR
    let base = fake(&c, &head);
    let r = Both(adr(&c, &head), &base);
    drive(&c, &r);
    assert!(brief(&c).contains("# ROLE: ARCHITECT") && brief(&c).contains(note));
    assert_eq!((state(&c, 12).as_str(), spawned(&c, "architect")), ("design", 2));
    drive(&c, &r);
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q2", "reject"]).0, 0);
    assert_eq!(kinds(&c, "design-rejected")[0].str("role"), Some("architect"));
    assert_eq!(state(&c, 12), "needs-human");
    drive(&c, &r);
    assert_eq!((spawned(&c, "architect"), spawned(&c, "impl")), (2, 0));
}

#[test]
fn a_superseded_design_lane_over_a_pushed_design_hands_it_off_once() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut c = setup("design-supersede", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    // Lane 1 pushes its design while it is still at it, so no hand-off yet.
    let working = Fake::default().reply(
        "herdr agent list",
        &agents(&[("sendmeter-design-12", "working", &wt(&c, 12), "w9:c1")]),
    );
    drive(&c, &fake(&c, HEAD));
    drive(&c, &Both(working, &fake(&c, HEAD)));
    assert_eq!(state(&c, 12), "design");
    // Lane 2 replaces it with nothing new to push: the design on the branch is still handed off, once.
    c.now += 1000;
    let r = fake(&c, HEAD);
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "design", "--supersede"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(spawned(&c, "design"), 2);
    let out = drive(&c, &r);
    assert!(
        out.contains(&format!("sendmeter #12 design-review: {HEAD:.7}")),
        "{out}"
    );
    assert_eq!(
        (state(&c, 12).as_str(), kinds(&c, "needs-decision").len()),
        ("design-review", 1)
    );
    drive(&c, &r);
    assert_eq!(kinds(&c, "design-review").len(), 1);
    // A revise lane with no new commit leaves the head already handed off alone.
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "make it blue"]).0, 0);
    c.now += 1000;
    drive(&c, &r);
    drive(&c, &r);
    assert_eq!(
        (state(&c, 12).as_str(), kinds(&c, "design-review").len()),
        ("design", 1)
    );
}
