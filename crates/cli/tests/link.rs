mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Duplex, Out, Runner};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;
const COST_MS: u64 = 700;
const BOATS: [&str; 3] = ["boat-1", "boat-2", "boat-3"];
const EXE: &str = "/opt/drover/bin/drover";

static CLOCK: AtomicU64 = AtomicU64::new(NOW);

fn clock() -> u64 {
    CLOCK.load(Relaxed)
}

#[derive(Default)]
struct Stream {
    gone: AtomicBool,
    mute: AtomicBool,
}

struct Pipe(Option<std::io::PipeWriter>, Arc<Stream>);

impl Write for Pipe {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.1.gone.load(Relaxed) {
            self.0 = None;
        }
        if self.1.mute.load(Relaxed) {
            return Ok(b.len());
        }
        match &mut self.0 {
            Some(w) => w.write(b),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.as_mut().map_or(Ok(()), Write::flush)
    }
}

struct Boats {
    fake: &'static Fake,
    sheds: BTreeMap<&'static str, &'static FakeShed>,
    spent_ms: AtomicU64,
    calls: Mutex<BTreeMap<String, usize>>,
    streams: Mutex<BTreeMap<String, Arc<Stream>>>,
    refused: Mutex<Vec<String>>,
    kicks: Mutex<Vec<(Instant, u64)>>,
}

impl Boats {
    fn charge(&self, host: &str) {
        self.spent_ms.fetch_add(COST_MS, Relaxed);
        *self.calls.lock().unwrap().entry(host.into()).or_default() += 1;
    }
    fn calls(&self) -> BTreeMap<String, usize> {
        self.calls.lock().unwrap().clone()
    }
    fn on(&self, host: &str) -> drover::shed::With<'static> {
        drover::shed::With(self.fake, self.sheds[host].shed())
    }
    fn cut(&self, host: &str) {
        self.streams.lock().unwrap()[host].gone.store(true, Relaxed);
    }
    fn mute(&self, host: &str, on: bool) {
        self.streams.lock().unwrap()[host].mute.store(on, Relaxed);
    }
    fn refuse(&self, host: &str, on: bool) {
        let mut r = self.refused.lock().unwrap();
        r.retain(|h| h != host);
        if on {
            r.push(host.into());
        }
    }
    fn kicked_at(&self, issue: u64, after: Instant) -> Option<Instant> {
        let k = self.kicks.lock().unwrap();
        k.iter().find(|(t, n)| *n == issue && *t >= after).map(|k| k.0)
    }
}

fn host_of(prog: &str, args: &[String]) -> Option<String> {
    let end = args.iter().position(|a| a == "--").filter(|_| prog == "ssh")?;
    args.get(end.checked_sub(1)?).cloned()
}

impl Runner for Boats {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if let Some(h) = host_of(prog, args) {
            self.charge(&h);
        }
        let script = args.get(1).filter(|_| prog == "sh");
        if let Some(n) = script.and_then(|s| s.split(" kick sendmeter ").nth(1)?.split(' ').next()?.parse().ok()) {
            self.kicks.lock().unwrap().push((Instant::now(), n));
        }
        self.fake.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        let h = host_of(prog, args).filter(|_| args.last().is_some_and(|a| a.ends_with("'hosts' 'report'")));
        let Some(h) = h else { return self.run(prog, args) };
        self.charge(&h);
        let mut stdout = vec![];
        drover::link::watch::serve(&self.on(&h), &mut input.as_bytes(), &mut stdout)?;
        Ok(Out {
            stdout: String::from_utf8_lossy(&stdout).into(),
            ..Out::default()
        })
    }
    fn sleep(&self, _: u64) {}
    fn pipe(&self, prog: &str, args: &[String]) -> anyhow::Result<Duplex> {
        let watch = args.last().is_some_and(|a| a.ends_with("'hosts' 'watch'"));
        let h = match host_of(prog, args) {
            Some(h) if watch => h,
            None if prog == EXE && args == ["hosts", "watch"] => "mini".into(),
            _ => anyhow::bail!("not a watcher: {prog} {args:?}"),
        };
        self.charge(&h);
        if self.refused.lock().unwrap().contains(&h) {
            anyhow::bail!("ssh: connect to host {h}: Operation timed out");
        }
        let s = Arc::new(Stream::default());
        self.streams.lock().unwrap().insert(h.clone(), s.clone());
        let (mut stdin, to_boat) = std::io::pipe()?;
        let (from_boat, out) = std::io::pipe()?;
        let on = self.on(&h);
        std::thread::spawn(move || drover::link::watch::watch(&on, &mut stdin, &mut Pipe(Some(out), s)));
        Ok((Box::new(to_boat), Box::new(from_boat)))
    }
}

struct Fleet {
    c: drover::Ctx,
    r: &'static Boats,
    lanes: Vec<(u64, PathBuf)>,
    _one: std::sync::MutexGuard<'static, ()>,
}

impl Drop for Fleet {
    fn drop(&mut self) {
        for h in std::iter::once("mini").chain(BOATS) {
            self.r.refuse(h, true);
        }
        self.r
            .streams
            .lock()
            .unwrap()
            .values()
            .for_each(|s| s.gone.store(true, Relaxed));
    }
}

fn fleet(name: &str, limits: &str) -> Fleet {
    static ONE: Mutex<()> = Mutex::new(());
    let one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let d = tmpdir(name);
    let mut sheds: BTreeMap<&'static str, &'static FakeShed> = BTreeMap::new();
    for h in std::iter::once("mini").chain(BOATS) {
        sheds.insert(h, Box::leak(Box::new(FakeShed::new())));
    }
    let mut cfg = sheds["mini"].config(&CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""));
    cfg = cfg.replace("[limits]\n", &format!("[limits]\n{limits}\n"));
    cfg += "\n[hosts.mini]\n";
    for b in BOATS {
        cfg += &format!("\n[hosts.{b}]\nmachine = \"{b}\"\n");
    }
    let c = ctx(&d, &cfg);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let mut lanes = vec![];
    for n in 1..=30u64 {
        let b = BOATS[usize::try_from(n % 3).unwrap()];
        let wt = d.join(format!("{b}/sendmeter/issue-{n}"));
        std::fs::create_dir_all(wt.join(".drover")).unwrap();
        let agent = format!("sendmeter-impl-{n}");
        let d = json!({"role": "impl", "agent": agent, "harness": "claude", "pane": agent, "runtime": "shed",
                       "worktree": wt, "branch": format!("issue-{n}"), "host": b, "machine": b});
        append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(n), "spawn", d)).unwrap();
        sheds[b].lane(&agent, "working", &wt.to_string_lossy());
        lanes.push((n, wt));
    }
    let head = format!("{} {}\n", "a".repeat(40), NOW - 600);
    let r = Boats {
        fake: Box::leak(Box::new(Fake::default().reply("git -C", &head))),
        sheds,
        spent_ms: AtomicU64::new(0),
        calls: Mutex::default(),
        streams: Mutex::default(),
        refused: Mutex::default(),
        kicks: Mutex::default(),
    };
    CLOCK.store(NOW, Relaxed);
    Fleet {
        c,
        r: Box::leak(Box::new(r)),
        lanes,
        _one: one,
    }
}

impl Fleet {
    fn link(&self, h: &'static str) {
        let (c, r) = (self.c.clone(), self.r);
        let m = (h != "mini").then_some(h);
        std::thread::spawn(move || drover::link::run(&c, r, (h, m), clock));
    }
    fn links(&self) {
        for h in BOATS {
            self.link(h);
        }
    }
    fn now(&self) -> drover::Ctx {
        drover::Ctx {
            now: clock(),
            ..self.c.clone()
        }
    }
    fn view(&self) -> drover::link::view::View {
        let cfg = drover::config::Config::load(&self.c.config).unwrap();
        drover::link::view::View::load(&self.now(), &cfg).unwrap()
    }
    fn live(&self, h: &str) -> bool {
        self.view().live(h)
    }
    fn write(&self, n: u64, args: &[&str]) -> (u64, Instant) {
        static SENT: AtomicU64 = AtomicU64::new(1);
        let wt = &self.lanes.iter().find(|l| l.0 == n).unwrap().1;
        let sent = SENT.fetch_add(1, Relaxed);
        let line = json!({"args": args, "lane": format!("sendmeter:{n}:impl"), "ts": sent});
        let mut f = std::fs::File::options()
            .create(true)
            .append(true)
            .open(wt.join(".drover/outbox.jsonl"))
            .unwrap();
        writeln!(f, "{line}").unwrap();
        (sent, Instant::now())
    }
    fn ran(&self) -> Vec<(String, u64)> {
        let ev = read(&self.c.state).unwrap();
        let ran = ev
            .iter()
            .filter(|e| e.kind == "outbox" && e.data.get("refused").is_none());
        ran.map(|e| (e.str("lane").unwrap().to_string(), e.val("sent").as_u64().unwrap()))
            .collect()
    }
    fn turns(&self) -> usize {
        read(&self.c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "turn-end")
            .count()
    }
}

fn until(what: &str, secs: u64, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < end, "not within {secs} s: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn once_each(f: &Fleet, want: usize) {
    until(&format!("{want} lines ran"), 30, || f.ran().len() >= want);
    let ran = f.ran();
    let mut seen = ran.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), ran.len(), "a line ran twice: {ran:?}");
    assert_eq!(ran.len(), want, "{ran:?}");
    assert_eq!(f.turns(), want);
}

#[test]
fn a_turn_on_a_boat_starts_its_job_within_5s_p95_over_100() {
    let f = fleet("link-act", "");
    f.links();
    until("every boat's link is live", 10, || BOATS.iter().all(|h| f.live(h)));
    let mut lags = vec![];
    for round in 0..10u64 {
        let wrote: Vec<_> = (1..=10)
            .map(|i| (round * 10 + i) % 30 + 1)
            .map(|n| (n, f.write(n, &["turn", "working"]).1))
            .collect();
        until("every turn kicked its issue", 30, || {
            wrote.iter().all(|(n, t)| f.r.kicked_at(*n, *t).is_some())
        });
        lags.extend(
            wrote
                .iter()
                .map(|(n, t)| f.r.kicked_at(*n, *t).unwrap().duration_since(*t).as_millis()),
        );
    }
    lags.sort_unstable();
    let (p50, p95) = (lags[lags.len() / 2], lags[lags.len() * 95 / 100 - 1]);
    eprintln!(
        "#1319: turn on a boat to its kick over a live link, 100 lines: p50 {p50} ms, p95 {p95} ms, max {} ms",
        lags[99]
    );
    assert!(p95 <= 5_000, "p95 {p95} ms (target 5 s)");
    once_each(&f, 100);
}

#[test]
fn the_service_links_every_host_and_a_pass_on_live_links_calls_none() {
    let f = fleet("link-service", "");
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (c, r) = (f.c.clone(), f.r);
    std::thread::spawn(move || drover_service::run(c, r, clock, api));
    let all = || std::iter::once(&"mini").chain(BOATS.iter()).all(|h| f.live(h));
    until("the service's link to the main host and each boat is live", 10, all);
    let before = f.r.calls();
    let (code, out) = run(&f.now(), f.r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(f.r.calls(), before, "a pass on live links calls no host");
    let cfg = drover::config::Config::load(&f.c.config).unwrap();
    let rows = drover::timer::services(&f.now(), &cfg, f.r, None);
    let links = rows.iter().find(|r| r.0 == "links").unwrap();
    assert_eq!((links.1, links.2.as_str()), ("g", "4 of 4 live"));
}

#[test]
fn a_hundred_lines_across_three_cut_streams_run_once_each() {
    let f = fleet(
        "link-cut",
        "link_stale_secs = 2\nlink_beat_secs = 1\nlink_retry_max_secs = 1",
    );
    f.links();
    until("every boat's link is live", 10, || BOATS.iter().all(|h| f.live(h)));
    for i in 0..100u64 {
        f.write(i % 30 + 1, &["turn", "working"]);
        if let Some(b) = [20, 50, 80].iter().position(|at| *at == i) {
            f.r.cut(BOATS[b]);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    once_each(&f, 100);
    let dials: usize = BOATS.iter().map(|b| f.r.calls()[*b]).sum();
    assert!(dials >= 6, "each cut stream was dialled again: {:?}", f.r.calls());
}

#[test]
fn a_sleep_a_reboot_and_a_half_open_link_each_come_level_on_redial() {
    let f = fleet(
        "link-level",
        "link_stale_secs = 2\nlink_beat_secs = 1\nlink_retry_max_secs = 1",
    );
    f.links();
    until("every boat's link is live", 10, || BOATS.iter().all(|h| f.live(h)));
    let boot = |h: &str| f.view().link(h).unwrap().boot.clone();

    f.r.refuse("boat-1", true);
    f.r.cut("boat-1");
    until("boat-1's link is down", 10, || !f.live("boat-1"));
    for n in [3, 6, 9] {
        f.write(n, &["turn", "working"]);
    }
    CLOCK.fetch_add(600, Relaxed);
    assert!(f.ran().is_empty(), "nothing ran while the main host slept");
    f.r.refuse("boat-1", false);
    once_each(&f, 3);
    until("boat-1 is live after the sleep", 10, || f.live("boat-1"));

    let was = boot("boat-2");
    f.r.cut("boat-2");
    f.r.sheds["boat-2"].exit("sendmeter-impl-4");
    until("boat-2 is back on a new boot", 10, || {
        f.live("boat-2") && boot("boat-2") != was
    });
    let gone = |e: &&Event| {
        e.kind == "lane-state" && e.str("agent") == Some("sendmeter-impl-4") && e.str("state") == Some("gone")
    };
    until("the rebooted boat's dead lane is stamped once", 10, || {
        read(&f.c.state).unwrap().iter().any(|e| gone(&e))
    });
    assert_eq!(read(&f.c.state).unwrap().iter().filter(gone).count(), 1);

    let was = boot("boat-3");
    f.r.mute("boat-3", true);
    f.write(5, &["turn", "working"]);
    until("the half-open link is cut", 10, || {
        boot("boat-3") != was || !f.live("boat-3")
    });
    f.r.mute("boat-3", false);
    until("boat-3 is live again", 10, || f.live("boat-3") && boot("boat-3") != was);
    once_each(&f, 4);
}

#[test]
fn with_a_link_down_a_pass_polls_that_host_as_before() {
    let f = fleet(
        "link-down",
        "link_stale_secs = 2\nlink_beat_secs = 1\nlink_retry_max_secs = 1",
    );
    f.links();
    until("every boat's link is live", 10, || BOATS.iter().all(|h| f.live(h)));
    f.r.refuse("boat-2", true);
    f.r.cut("boat-2");
    until("boat-2's link is down", 10, || !f.live("boat-2"));
    f.write(4, &["turn", "working"]);
    std::thread::sleep(Duration::from_secs(2));
    assert!(f.ran().is_empty(), "no link carries boat-2's line");
    let before = f.r.calls();
    let (code, out) = run(&f.now(), f.r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(f.ran().len(), 1, "the pass polled boat-2 and ran its line");
    let after = f.r.calls();
    let polled = |h: &str| after[h] - before[h];
    assert_eq!(
        (polled("boat-1"), polled("boat-3")),
        (0, 0),
        "live links are not polled: {before:?} {after:?}"
    );
    once_each(&f, 1);
}
