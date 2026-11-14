mod common;
use common::*;
use drover::events::{fold, read, Event};
use drover::runner::{Out, Real, Runner};
use drover::serve::Server;
use std::path::Path;
use std::sync::Mutex;

static ONE: Mutex<()> = Mutex::new(());
const ADDR: &str = "127.0.0.1:7777";
const DESIGN: &str = "\n[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";
const HEAD: &str = "dddddddddddddddddddddddddddddddddddddddd";

fn setup(name: &str, extra: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), &format!("{CONFIG}{DESIGN}{extra}"));
    let sh = FakeShed::new();
    sh.attach(&c);
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    (c, sh)
}

fn wt(c: &drover::Ctx, n: u64) -> String {
    c.worktrees.join(format!("sendmeter/issue-{n}")).display().to_string()
}

fn fake(c: &drover::Ctx, head: &str) -> Fake {
    let w = wt(c, 12);
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply(&format!("git -C {w} log -1"), &format!("{head}\n"))
        .reply(&format!("git -C {w} rev-list"), "0\n")
        .reply(
            &format!("git -C {w} log --format= --name-only"),
            "docs/design/meter/index.html\ndocs/design/meter/1440.png\ndocs/design/meter/390.png\n",
        )
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("shed send orchestrator-t1", "")
}

fn drive(c: &drover::Ctx, r: &dyn Runner) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn state(c: &drover::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn spawned(c: &drover::Ctx, role: &str) -> usize {
    kinds(c, "spawn").iter().filter(|e| e.str("role") == Some(role)).count()
}

#[test]
fn a_marked_issue_gets_design_until_approved_and_an_unmarked_one_goes_straight_to_impl() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, sh) = setup("design-flow", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "13"]).0, 0);
    let r = fake(&c, HEAD);
    drive(&c, &r);
    assert!(sh.called("run", "sendmeter-design-12") && !sh.called("run", "sendmeter-impl-12"));
    assert!(sh.called("run", "sendmeter-impl-13"), "unmarked: impl at once");
    let wt12 = format!(
        "git -C /tmp/does-not-matter/sendmeter worktree add {} issue-12",
        wt(&c, 12)
    );
    assert!(r.called(&wt12), "{:?}", r.calls.borrow());
    assert_eq!((state(&c, 12).as_str(), state(&c, 13).as_str()), ("design", "impl"));
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
    assert!(w.contains("drover answer sendmeter q1 approve|revise|reject"), "{w}");
    drive(&c, &r);
    assert_eq!(spawned(&c, "impl"), 1, "never impl before the approval");
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "approve"]).0, 0);
    assert_eq!(kinds(&c, "design-approved").len(), 1);
    assert!(sh.called("kill", "sendmeter-design-12"), "the design pane goes");
    let busy = fake(&c, HEAD).once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    drive(&c, &busy);
    let p = drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12).expect("pending");
    assert_eq!((p.action.as_str(), p.reason.as_deref()), ("approve", Some("HOST_BUSY")));
    assert_eq!(spawned(&c, "impl"), 1, "only #13's");
    drive(&c, &r);
    assert!(sh.called("run", "sendmeter-impl-12"));
    assert_eq!((state(&c, 12).as_str(), spawned(&c, "design")), ("impl", 1));
    assert_eq!(drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12), None);
}

#[test]
fn a_design_lane_whose_only_untracked_file_is_its_claude_local_md_hands_off() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, _sh) = setup("design-claude-local", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let mut r = fake(&c, HEAD);
    let st = Out {
        code: 0,
        stdout: "?? CLAUDE.local.md\n".into(),
        stderr: String::new(),
    };
    r.replies.insert(0, (format!("git -C {} status", wt(&c, 12)), st));
    drive(&c, &r);
    assert_eq!(state(&c, 12), "design");
    let out = drive(&c, &r);
    assert!(
        out.contains(&format!("sendmeter #12 design-review: {:.7}", HEAD)),
        "{out}"
    );
    assert_eq!(
        (state(&c, 12).as_str(), kinds(&c, "needs-decision").len()),
        ("design-review", 1)
    );
}

fn post(s: &Server, path: &str, body: &str, cookie: &str, now: u64) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

fn needs_and_chat(s: &Server, cookie: &str, now: u64) -> (String, String) {
    let [needs, _, chat] = parts(s, ADDR, cookie, now);
    (needs, chat)
}

#[test]
fn the_board_card_and_the_thread_ask_are_one_answer_first_tap_wins() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, _sh) = setup(
        "design-surfaces",
        "\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n",
    );
    let r = fake(&c, HEAD);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    assert!(post(&s, "/thread", "op=new&repo=sendmeter&text=Design+first", &cookie, c.now).contains("t1"));
    std::env::set_var("DROVER_THREAD", "t1");
    let q = run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "12", "13", "--design"],
    );
    std::env::remove_var("DROVER_THREAD");
    assert_eq!(q.0, 0, "{}", q.1);
    let png = Path::new(&wt(&c, 12)).join("docs/design/meter");
    std::fs::create_dir_all(&png).unwrap();
    std::fs::write(png.join("1440.png"), b"\x89PNG wide").unwrap();
    drive(&c, &r);
    drive(&c, &r);
    let ask = &kinds(&c, "needs-decision")[0];
    assert_eq!((ask.issue, ask.str("thread")), (Some(12), Some("t1")));
    let (needs, chat) = needs_and_chat(&s, &cookie, c.now + 5);
    assert!(
        needs.contains("<details class=ask>") && needs.contains("<img src=\"design/sendmeter/12/0\""),
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
        !needs.contains("<details class=ask>") && !chat.contains("data-id=\"q1\""),
        "{needs}"
    );
    assert!(chat.contains("✓ 1. approve"), "{chat}");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "reject"]);
    assert!(code == 3 && out.contains("REFUSED ALREADY_ANSWERED"), "{out}");
    std::fs::create_dir_all(wt(&c, 13)).unwrap();
    let w = wt(&c, 13);
    let r13 = Fake::default()
        .reply(&format!("git -C {w} log -1"), &format!("{}\n", "e".repeat(40)))
        .reply(
            &format!("git -C {w} log --format= --name-only"),
            "docs/design/meter/1440.png\n",
        )
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
    let (mut c, _sh) = setup("design-revise", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    for k in 1..=5 {
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

fn repo(d: &Path) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let (origin, local) = (d.join("origin.git"), d.join("local"));
    git(d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    git(&local, &["config", "user.email", "t@example.com"]);
    git(&local, &["config", "user.name", "t"]);
    std::fs::write(local.join("app.rs"), "fn main() {}\n").unwrap();
    git(&local, &["add", "."]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "origin", "HEAD:staging"]);
    let cfg = format!("{CONFIG}{DESIGN}").replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(d, &cfg);
    let sh = FakeShed::attached(&c);
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
    (c, w, sh)
}

#[test]
fn reject_pushes_one_commit_back_to_the_exact_pre_design_tree_and_needs_a_human() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let d = tmpdir("design-reject");
    let (c, w, _sh) = repo(&d);
    let origin = d.join("origin.git");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let r = RealGit(fake(&c, HEAD));
    drive(&c, &r);
    let pre = git(&w, &["rev-parse", "HEAD"]);
    assert_eq!(kinds(&c, "spawn")[0].str("sha"), Some(pre.as_str()));
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
    let (c, sh) = setup("design-off", "");
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
    assert!(sh.called("run", "sendmeter-impl-12") && !sh.called("run", "sendmeter-design-12"));
    assert_eq!(state(&c, 12), "impl");
}

#[test]
fn a_paused_fleet_auto_spawns_no_design_but_the_orchestrator_still_can() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, sh) = setup("design-paused", "");
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
    assert!(sh.all("run").is_empty(), "{:?}", sh.methods());
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "design"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("run", "sendmeter-design-12"));
    assert_eq!(state(&c, 12), "design");
}

const ARCHITECT: &str = "\n[fleet.sendmeter.role.architect]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";

fn adr(c: &drover::Ctx, head: &str) -> Fake {
    let w = wt(c, 12);
    Fake::default().reply(
        &format!("git -C {w} log --format= --name-only {head}..{head} ^origin/staging -- docs/architecture"),
        "docs/architecture/meter/adr.md\ndocs/architecture/meter/statechart.md\n",
    )
}

fn brief(c: &drover::Ctx) -> String {
    std::fs::read_to_string(Path::new(&wt(c, 12)).join(".brief.md")).unwrap()
}

#[test]
fn design_and_architect_run_architect_then_design_then_impl_which_links_the_adr() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, sh) = setup("architect-flow", ARCHITECT);
    let q = ["queue", "sendmeter", "add", "12", "--design", "--architect"];
    assert_eq!(run(&c, &Fake::default(), &q).0, 0);
    let base = fake(&c, HEAD);
    let r = Both(adr(&c, HEAD), &base);
    drive(&c, &r);
    assert!(sh.called("run", "sendmeter-architect-12") && !sh.called("run", "sendmeter-design-12"));
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
    assert!(sh.called("run", "sendmeter-design-12"), "the design comes next");
    assert_eq!((state(&c, 12).as_str(), spawned(&c, "impl")), ("design", 0));
    let r = fake(&c, &"e".repeat(40));
    drive(&c, &r);
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q2", "approve"]).0, 0);
    drive(&c, &r);
    assert!(sh.called("run", "sendmeter-impl-12"));
    assert_eq!(state(&c, 12), "impl");
    let b = brief(&c);
    assert!(
        b.contains("the owner approved the ADR in `docs/architecture/meter/` on this branch; build to it, and add any `.drover/arch.toml` lines it ends with in this PR."),
        "{b}"
    );
}

#[test]
fn architect_revise_reruns_it_with_the_note_and_reject_needs_a_human() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut c, _sh) = setup("architect-revise", ARCHITECT);
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
    let head = "e".repeat(40);
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
    let (mut c, sh) = setup("design-supersede", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    drive(&c, &fake(&c, HEAD));
    sh.lane("sendmeter-design-12", "working", &wt(&c, 12));
    drive(&c, &fake(&c, HEAD));
    assert_eq!(state(&c, 12), "design");
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
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "make it blue"]).0, 0);
    c.now += 1000;
    drive(&c, &r);
    drive(&c, &r);
    assert_eq!(
        (state(&c, 12).as_str(), kinds(&c, "design-review").len()),
        ("design", 1)
    );
}

#[test]
fn a_design_brief_takes_the_medium_and_path_the_issue_names() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, _sh) = setup("design-medium", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let body = r#"{"title":"t","body":"Draw SwiftUI views in `apple/design/x/` (previews only)."}"#;
    let base = fake(&c, HEAD);
    let r = Both(Fake::default().reply("gh issue view", body), &base);
    drive(&c, &r);
    let b = brief(&c);
    assert!(
        b.contains("- Medium: SwiftUI. You write only under `docs/design/`, `apple/design/x/`;"),
        "{b}"
    );
    drive(&c, &r);
    let w = wt(&c, 12);
    assert!(base.called(&format!(
        "git -C {w} log --format= --name-only {HEAD}..{HEAD} ^origin/staging -- docs/design apple/design/x"
    )));
    assert_eq!(state(&c, 12), "design-review");
    let (c, _sh) = setup("design-medium-html", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    drive(&c, &fake(&c, HEAD));
    assert!(brief(&c).contains("- Medium: HTML. You write only under `docs/design/`;"));
}

#[test]
fn a_design_pushed_under_the_issues_named_path_raises_the_ask_and_one_elsewhere_stays_idle() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = r#"{"title":"t","body":"Draw SwiftUI views; screenshots in `apple/design/x/`."}"#;
    for (name, files) in [
        ("design-named", ["apple/design/x/iphone.png", "apple/design/x/Mac.png"]),
        ("design-elsewhere", ["apple/Sources/A.swift", "apple/Sources/a.png"]),
    ] {
        let (c, w, _sh) = repo(&tmpdir(name));
        assert_eq!(
            run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
            0
        );
        let base = RealGit(fake(&c, HEAD));
        let r = Both(Fake::default().reply("gh issue view", body), &base);
        drive(&c, &r);
        for f in files {
            std::fs::create_dir_all(w.join(f).parent().unwrap()).unwrap();
            std::fs::write(w.join(f), "png").unwrap();
        }
        git(&w, &["add", "apple"]);
        git(&w, &["commit", "-qm", "design"]);
        git(&w, &["push", "-q", "origin", "issue-12"]);
        drive(&c, &r);
        let (_, waiting) = run(&c, &r, &["waiting", "sendmeter"]);
        let asks = kinds(&c, "needs-decision");
        if name == "design-elsewhere" {
            assert_eq!(state(&c, 12), "design");
            assert!(asks.is_empty() && kinds(&c, "design-review").is_empty());
            assert!(
                waiting.contains("design lane idle without a pushed design"),
                "{waiting}"
            );
            continue;
        }
        assert_eq!(state(&c, 12), "design-review");
        let review = &kinds(&c, "design-review")[0];
        for e in [review, &asks[0]] {
            let mut got: Vec<&str> = e.data["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["name"].as_str().unwrap())
                .collect();
            got.sort_unstable();
            assert_eq!(got, ["apple/design/x/Mac.png", "apple/design/x/iphone.png"]);
        }
        assert!(
            waiting.contains("drover answer sendmeter q1 approve|revise|reject"),
            "{waiting}"
        );
    }
}

const SWIFT: &str = "# Design recipe\n\n- medium: SwiftUI\n- folder: apple/design\n- render: `just design-shots`\n\
- sizes: iPhone 393x852, Mac 1440x900\n";

#[test]
fn a_design_brief_takes_the_repos_recipe_unless_the_issue_names_its_own_path() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (name, body, want) in [
        (
            "design-recipe",
            "b",
            "- Medium: SwiftUI. You write only under `apple/design/`;",
        ),
        (
            "design-recipe-override",
            "UIKit screens in `ios/design/y/`.",
            "- Medium: UIKit. You write only under `docs/design/`, `ios/design/y/`;",
        ),
    ] {
        let (c, _sh) = setup(name, "");
        assert_eq!(
            run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
            0
        );
        let base = fake(&c, HEAD);
        let issue = format!(r#"{{"title":"t","body":"{body}"}}"#);
        let recipe = Fake::default().reply("git -C /tmp/does-not-matter/sendmeter show", SWIFT);
        let r = Both(recipe.reply("gh issue view", &issue), &base);
        drive(&c, &r);
        let b = brief(&c);
        assert!(b.contains(want), "{b}");
        let shots = "- Images: run `just design-shots` at iPhone 393x852, Mac 1440x900.";
        assert_eq!(b.contains(shots), name == "design-recipe", "{b}");
    }
    let (c, _sh) = setup("design-no-recipe", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    drive(&c, &fake(&c, HEAD));
    let b = brief(&c);
    assert!(
        b.contains("- Medium: HTML. You write only under `docs/design/`;"),
        "{b}"
    );
    assert!(
        b.contains("- Images: render a PNG of each page or view at 1440 and 390 wide.\n"),
        "{b}"
    );
}

#[test]
fn a_recipe_with_a_clip_hands_off_the_images_and_the_clip() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, w, _sh) = repo(&tmpdir("design-clip"));
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let r = RealGit(fake(&c, HEAD));
    drive(&c, &r);
    std::fs::create_dir_all(w.join(".drover")).unwrap();
    let game = "- medium: Godot scene\n- folder: game/design\n- sizes: 1920x1080\n- clip: yes\n";
    std::fs::write(w.join(".drover/design.md"), game).unwrap();
    for f in [
        "game/design/x/title.png",
        "game/design/x/play.png",
        "game/design/x/clip.mp4",
    ] {
        std::fs::create_dir_all(w.join(f).parent().unwrap()).unwrap();
        std::fs::write(w.join(f), "bytes").unwrap();
    }
    git(&w, &["add", "."]);
    git(&w, &["commit", "-qm", "design"]);
    git(&w, &["push", "-q", "origin", "issue-12"]);
    drive(&c, &r);
    assert_eq!(state(&c, 12), "design-review");
    for e in [&kinds(&c, "design-review")[0], &kinds(&c, "needs-decision")[0]] {
        let mut got: Vec<&str> = e.data["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        got.sort_unstable();
        assert_eq!(
            got,
            [
                "game/design/x/clip.mp4",
                "game/design/x/play.png",
                "game/design/x/title.png"
            ]
        );
    }
}

#[test]
fn a_recipe_tool_the_main_host_lacks_ends_the_design_turn_blocked_before_it_starts() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, sh) = setup("design-tool", "");
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let base = fake(&c, HEAD);
    let need = format!("{SWIFT}- tools: xcode, blender\n");
    let r = Both(
        Fake::default()
            .reply("git -C /tmp/does-not-matter/sendmeter show", &need)
            .reply("sh -c uname -s", "Darwin\narm64\n/Users/o\nxcode\n"),
        &base,
    );
    drive(&c, &r);
    let turn = &kinds(&c, "turn-end")[0];
    assert_eq!((turn.str("state"), turn.str("role")), (Some("blocked"), Some("design")));
    let why = turn.str("why").unwrap();
    assert!(why.starts_with("missing tool blender:"), "{why}");
    let card = &kinds(&c, "needs-human")[0];
    assert_eq!(card.str("code"), Some("LANE_BLOCKED"));
    assert!(card.str("reason").unwrap().contains("missing tool blender"));
    assert!(!sh.called("run", "sendmeter-design-12") && spawned(&c, "design") == 0);
    assert!(!Path::new(&wt(&c, 12)).join(".brief.md").exists(), "nothing drawn");
    assert_eq!(state(&c, 12), "needs-human");
}

fn staged(name: &str, flag: &str) -> (drover::Ctx, std::path::PathBuf, RealGit, FakeShed) {
    let (c, w, sh) = repo(&tmpdir(name));
    let toml = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, format!("{toml}{ARCHITECT}")).unwrap();
    let t = run(&c, &Fake::default(), &["thread", "new", "sendmeter", "meter"]);
    assert!(t.0 == 0 && t.1.contains("t1"), "{}", t.1);
    std::env::set_var("DROVER_THREAD", "t1");
    let q = run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", flag]);
    std::env::remove_var("DROVER_THREAD");
    assert_eq!(q.0, 0, "{}", q.1);
    sh.add(
        "orchestrator-t1",
        serde_json::json!({"fleet": "sendmeter", "role": "orchestrator", "thread": "t1"}),
    );
    let st = serde_json::json!({"thread": "t1", "agent": "orchestrator-t1", "pane": "orchestrator-t1", "runtime": "shed-acp", "harness": "claude"});
    drover::events::append(
        &c.state,
        &Event::new(c.now, "sendmeter", None, "orchestrator-start", st),
    )
    .unwrap();
    let r = RealGit(fake(&c, HEAD));
    drive(&c, &r);
    (c, w, r, sh)
}

fn push(w: &Path, files: &[(&str, &str)]) -> String {
    for (f, text) in files {
        std::fs::create_dir_all(w.join(f).parent().unwrap()).unwrap();
        std::fs::write(w.join(f), text).unwrap();
    }
    git(w, &["add", "."]);
    git(w, &["commit", "-qm", "stage"]);
    git(w, &["push", "-q", "origin", "issue-12"]);
    git(w, &["rev-parse", "HEAD"])
}

fn tree(head: &str, dir: &str) -> String {
    format!("https://github.com/jirathip-dev/sendmeter/tree/{head}/{dir}/")
}

#[test]
fn an_architect_ask_quotes_the_summary_and_links_the_adr_folder_at_the_sha_and_a_revise_the_new_one() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut c, w, r, _sh) = staged("architect-summary", "--architect");
    let sum = "Decision: one store, synced nightly\nSlices: store, sync, board\nOpen questions: none";
    let readme = format!("# Meter\n\n## Summary\n\n{sum}\n\n## Shapes\n\n`struct Meter`\n");
    let head = push(
        &w,
        &[
            ("docs/architecture/meter/README.md", &readme),
            ("docs/architecture/meter/adr.md", "# ADR\n\n## Decision\n\nnot this\n"),
        ],
    );
    drive(&c, &r);
    let link = tree(&head, "docs/architecture/meter");
    let q = format!(
        "{sum}\n\n{link}\n\nArchitecture (ADR) for #12 is ready ({:.7}): approve, revise (type a note) or reject?",
        head
    );
    let ask = &kinds(&c, "needs-decision")[0];
    assert_eq!(ask.str("question"), Some(q.as_str()));
    assert_eq!((ask.str("summary"), ask.str("link")), (Some(sum), Some(link.as_str())));
    let (code, asks) = run(&c, &r, &["asks", "sendmeter"]);
    assert!(code == 0 && asks.contains(sum) && asks.contains(&link), "{asks}");
    let (code, thread) = run(&c, &r, &["thread", "show", "t1"]);
    assert!(
        code == 0 && thread.contains(&format!("asks q1: {sum}\n\n{link}")),
        "{thread}"
    );
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "split the sync"]).0, 0);
    c.now += 1000;
    drive(&c, &r);
    let readme = readme.replace("synced nightly", "synced hourly");
    let head2 = push(&w, &[("docs/architecture/meter/README.md", &readme)]);
    drive(&c, &r);
    let ask = &kinds(&c, "needs-decision")[1];
    let q = ask.str("question").unwrap();
    assert!(
        q.starts_with("Decision: one store, synced hourly\n") && q.contains(&tree(&head2, "docs/architecture/meter")),
        "{q}"
    );
}

#[test]
fn a_design_ask_links_the_design_folder_the_issue_names_at_the_sha() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, w, _sh) = repo(&tmpdir("design-link"));
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let body = r#"{"title":"t","body":"Draw SwiftUI views; screenshots in `apple/design/x/`."}"#;
    let base = RealGit(fake(&c, HEAD));
    let r = Both(Fake::default().reply("gh issue view", body), &base);
    drive(&c, &r);
    let sum = "Decision: a single meter dial\nScreens: iPhone, Mac\nOpen questions: dark mode?";
    let head = push(
        &w,
        &[
            ("apple/design/x/iphone.png", "png"),
            (
                "apple/design/x/README.md",
                &format!("## Summary\n{sum}\n## Images\n- iphone.png\n"),
            ),
        ],
    );
    drive(&c, &r);
    let ask = &kinds(&c, "needs-decision")[0];
    let link = tree(&head, "apple/design/x");
    assert_eq!(ask.str("link"), Some(link.as_str()));
    let q = ask.str("question").unwrap();
    assert!(
        q.starts_with(&format!("{sum}\n\n{link}\n\nDesign for #12 is ready")),
        "{q}"
    );
}

#[test]
fn without_a_summary_the_ask_quotes_the_adrs_decision_and_without_either_it_reads_as_before() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let adr = "# ADR\n\n## Question\n\nhow?\n\n## Decision\n\nOne store.\n\n### Why\n\nsimple\n\n## Open questions\n\n- sync cadence\n\n## Consequences\n\nmore code\n";
    for (name, file, want) in [
        (
            "architect-decision",
            adr,
            "Decision: One store.\n\n### Why\n\nsimple\nOpen questions: - sync cadence\n\n",
        ),
        ("architect-bare", "# ADR\n\nwe store it once\n", ""),
    ] {
        let (c, w, r, _sh) = staged(name, "--architect");
        let head = push(&w, &[("docs/architecture/meter/adr.md", file)]);
        drive(&c, &r);
        let ask = &kinds(&c, "needs-decision")[0];
        let bare = format!("Architecture (ADR) for #12 is ready ({head:.7}): approve, revise (type a note) or reject?");
        let link = tree(&head, "docs/architecture/meter");
        let q = if want.is_empty() {
            bare
        } else {
            format!("{want}{link}\n\n{bare}")
        };
        assert_eq!(ask.str("question"), Some(q.as_str()), "{name}");
        assert_eq!(ask.str("link"), Some(link.as_str()));
    }
}

#[test]
fn a_long_summary_is_cut_at_a_line_end_near_600_characters() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, w, r, _sh) = staged("architect-cap", "--architect");
    let lines: Vec<String> = (0..20).map(|i| format!("{i:02} {}", "x".repeat(46))).collect();
    let readme = format!("## Summary\n{}\n", lines.join("\n"));
    push(&w, &[("docs/architecture/meter/README.md", &readme)]);
    drive(&c, &r);
    let sum = kinds(&c, "needs-decision")[0].str("summary").unwrap().to_string();
    assert_eq!(sum, format!("{}\n…", lines[..12].join("\n")));
}

#[test]
fn an_architect_lane_that_errors_with_no_commit_over_an_unrelated_adr_folder_raises_no_ask_but_a_card() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, w, r, _sh) = staged("architect-error", "--architect");
    let local = w.parent().unwrap().parent().unwrap().parent().unwrap().join("local");
    let routines = local.join("docs/architecture/routines");
    std::fs::create_dir_all(&routines).unwrap();
    std::fs::write(routines.join("adr.md"), "# Routines\n").unwrap();
    git(&local, &["add", "."]);
    git(&local, &["commit", "-qm", "routines adr"]);
    git(&local, &["push", "-q", "origin", "HEAD:staging"]);
    git(&w, &["reset", "-q", "--hard", "origin/staging"]);
    let agent = kinds(&c, "spawn")[0].str("agent").unwrap().to_string();
    let d = serde_json::json!({"agent": agent, "state": "error", "note": "no response"});
    drover::events::append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "lane-state", d)).unwrap();
    drive(&c, &r);
    assert!(kinds(&c, "design-review").is_empty() && kinds(&c, "needs-decision").is_empty());
    let card = &kinds(&c, "needs-human")[0];
    assert!(card.str("reason").unwrap().contains("no response"), "{card:?}");
    assert_eq!(state(&c, 12), "needs-human");
}

#[test]
fn a_hotspots_issues_architect_brief_carries_the_hot_files_playbook_and_profiles_sync_writes_it() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let claude = "\n[fleet.sendmeter.role.architect]\nharness = \"claude\"\nprofile = \"fleet-architect\"\n";
    let (c, _sh) = setup("architect-hotspots", claude);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let role = c.config.with_file_name("claude").join("fleet-architect/role.md");
    let synced = std::fs::read_to_string(role).unwrap();
    let classes = [
        "## Hot files",
        "**Central lists become folders:**",
        "**Generated files are regenerated, never hand-merged:**",
        "**Docs and CI are split per area:**",
        "The ADR ends with the exact `.drover/arch.toml` lines the impl lane adds in the same PR",
    ];
    for want in classes {
        assert!(synced.contains(want), "role.md lacks {want:?}");
    }
    let (c, _sh) = setup("architect-hotspots-brief", ARCHITECT);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "12", "--architect"]
        )
        .0,
        0
    );
    let title = r#"{"title":"hotspots: sendmeter week of 2026-11-09","body":"b"}"#;
    let base = fake(&c, HEAD);
    let r = Both(adr(&c, HEAD).reply("gh issue view", title), &base);
    drive(&c, &r);
    let b = brief(&c);
    for want in classes {
        assert!(b.contains(want), "brief lacks {want:?}\n{b}");
    }
    assert!(
        b.contains("- This is a hotspots issue: convert each file it lists"),
        "{b}"
    );
    let (c, _sh) = setup("architect-plain-brief", ARCHITECT);
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
    drive(&c, &Both(adr(&c, HEAD), &base));
    assert!(!brief(&c).contains("This is a hotspots issue"), "{}", brief(&c));
}
