//! `herdsman serve`, the phone page (docs/serve.md): std `TcpListener`, a thread per request (at most 16, then 503);
//! `/live` SSE clients get a thread each, fed by the 1 s watcher thread.
use crate::config::{Config, Limits, Role};
use crate::events::{age, append, fold, plain, read, Event, IssueState, States};
use crate::gate::{gate, refused, Refusal};
use crate::host::{self, Host};
use crate::runner::{Out, Runner};
use crate::{devices, gh, herdr, lanes, queue, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::io::ErrorKind::{TimedOut, WouldBlock};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SW: &str = include_str!("../assets/sw.js");
macro_rules! th {
    ($f:literal, $t:expr) => {
        (
            concat!("themes/", $f),
            $t,
            include_bytes!(concat!("../assets/themes/", $f)),
        )
    };
}
/// #57: served under `/a/<VER>/` as immutable (the SW precaches them); the page links them.
const ASSETS: &[(&str, &str, &[u8])] = &[
    ("serve.css", "text/css", include_bytes!("../assets/serve.css")),
    ("herd.css", "text/css", include_bytes!("../assets/herd.css")),
    ("serve.js", "text/javascript", include_bytes!("../assets/serve.js")),
    ("herd.js", "text/javascript", include_bytes!("../assets/herd.js")),
    // #172: the built-in themes; `herdsman serve` serves the active one at `/theme/<file>` (`theme`).
    th!("farm/sprites.svg", SVG),
    th!("farm/theme.json", JSON),
    th!("kitchen/sprites.svg", SVG),
    th!("kitchen/theme.json", JSON),
    th!("kitchen/scene.svg", SVG),
];
const SVG: &str = "image/svg+xml";
const I180: &[u8] = include_bytes!("../assets/icon-180.png");
const I192: &[u8] = include_bytes!("../assets/icon-192.png");
/// At the root, the fixed paths the manifest and iOS name: cached a day (`/sw.js`, VER filled in, is no-store).
const FILES: &[(&str, &str, &[u8])] = &[
    (
        "manifest.webmanifest",
        "application/manifest+json",
        include_bytes!("../assets/manifest.webmanifest"),
    ),
    ("icon-180.png", "image/png", I180),
    ("icon-192.png", "image/png", I192),
    ("icon-512.png", "image/png", include_bytes!("../assets/icon-512.png")),
    // #195: the root icons Safari probes; a 404 there falls back to another service's icon on the same host.
    ("apple-touch-icon.png", "image/png", I180),
    ("apple-touch-icon-precomposed.png", "image/png", I180),
    ("favicon.ico", "image/png", I192),
];
/// FNV-1a over the assets and root files at build time: their `/a/<hash8>/` path and the icons' `?v=<hash8>`.
const VER: u32 = {
    let (mut h, mut k, mut i) = (0x811c_9dc5u32, 0, 0);
    while k < ASSETS.len() + FILES.len() {
        let b = if k < ASSETS.len() {
            ASSETS[k].2
        } else {
            FILES[k - ASSETS.len()].2
        };
        while i < b.len() {
            h = (h ^ b[i] as u32).wrapping_mul(0x0100_0193);
            i += 1;
        }
        (k, i) = (k + 1, 0);
    }
    h
};

type Clients = Arc<Mutex<Vec<(Sender<String>, Option<String>)>>>;
type Cache = Arc<Mutex<BTreeMap<String, (u64, Option<String>)>>>;
/// A form op for `serve_on`'s job thread: (ctx, form, fleet).
type Job = (Ctx, BTreeMap<String, String>, String);
/// When this board started (the Services board row's uptime).
static STARTED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

pub fn serve(ctx: &Ctx, r: &dyn Runner, addr: &str, read_only: bool) -> Result<()> {
    let l = TcpListener::bind(addr)?;
    let mode = if read_only { "read-only view" } else { "phone page" };
    eprintln!("herdsman serve: {mode} on http://{}/", l.local_addr()?);
    serve_on(ctx, r, l, read_only)
}

#[allow(clippy::needless_pass_by_value)] // public API: the listener is handed over and lives as long as the server
pub fn serve_on(ctx: &Ctx, r: &dyn Runner, l: TcpListener, read_only: bool) -> Result<()> {
    let addr = (!read_only)
        .then(|| l.local_addr().map(|a| a.to_string()))
        .transpose()?;
    let (tx, rx) = channel();
    let s = Server {
        jobs: Some(tx),
        ..Server::new(ctx, r, addr)
    };
    let (h, w) = (
        s.host.clone(),
        Server {
            fresh: true,
            cache: s.cache.clone(),
            clients: s.clients.clone(),
            kick: s.kick.clone(),
            host: s.host.clone(),
            ..Server::new(ctx, r, s.addr.clone())
        },
    );
    let (s, busy) = (&s, &AtomicUsize::new(0));
    STARTED.get_or_init(|| ctx.now);
    std::thread::scope(|sc| {
        // #57: the host every 10 s (its 2 s vm_stat delta included) here, never on a request.
        sc.spawn(move || loop {
            let mut hs = host::sample(r);
            host::window(&ctx.state, &mut hs);
            *lock(&h) = hs;
            std::thread::sleep(Duration::from_secs(10));
        });
        sc.spawn(move || loop {
            w.tick();
            std::thread::sleep(Duration::from_secs(1));
        });
        // #140: drive and the needs-human ops run here, one at a time; the board toasts the result.
        sc.spawn(move || {
            for (c, f, fleet) in rx {
                let msg = json!(s.write(&c, &f).unwrap_or_else(|e| shown(&e)));
                s.kick.store(lock(&s.running).remove(&fleet), Relaxed);
                lock(&s.clients).retain(|c| c.0.send(format!("event: toast\ndata: {msg}\n\n")).is_ok());
            }
        });
        for mut st in l.incoming().flatten() {
            if busy.fetch_add(1, Relaxed) < 16 {
                sc.spawn(move || (drop(s.handle(st)), busy.fetch_sub(1, Relaxed)));
            } else {
                busy.fetch_sub(1, Relaxed);
                drop(st.write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n"));
            }
        }
    });
    Ok(())
}

/// `Server::cached` as a Runner (runner, cache, now, ttl): also `waiting::scan`'s per-issue `git log` on a request.
struct Memo<'a>(&'a dyn Runner, &'a Cache, u64, u64);
impl Runner for Memo<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        let (key, cache) = (format!("{prog} {}", args.join(" ")), || lock(self.1));
        let hit = cache()
            .get(&key)
            .filter(|(t, _)| self.2 >= *t && self.2 - t < self.3)
            .cloned();
        let (t, out) = hit.unwrap_or_else(|| {
            (
                self.2,
                self.0.run(prog, args).ok().filter(|o| o.code == 0).map(|o| o.stdout),
            )
        });
        cache().insert(key, (t, out.clone()));
        Ok(out.map_or(
            Out {
                code: 1,
                ..Out::default()
            },
            |stdout| Out {
                stdout,
                ..Out::default()
            },
        ))
    }
}

/// The mutex's data, even after a thread panicked holding it.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn hash(s: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// A menu as its card shows it (masked question and options): `/answer` refuses a tap on any other.
fn seen(q: &str, opts: &[String]) -> u64 {
    hash(&(lanes::mask(q), opts.iter().map(|o| lanes::mask(o)).collect::<Vec<_>>()))
}

/// The watcher's last look: the log's (size, mtime), the `herdr agent list` read (secs, hash), last render, per-(paired, fragment) hashes.
#[derive(Default)]
struct Watch {
    log: Option<(u64, Option<std::time::SystemTime>)>,
    herdr: (u64, u64),
    rendered: u64,
    /// The last `herd` push: (time, hash of the body).
    herd: (u64, u64),
    frags: BTreeMap<(bool, &'static str), u64>,
    /// #166: `drive.lock` was there: a pass starting or ending re-renders (`⟳ driving…`).
    driving: bool,
}

pub struct Server<'a> {
    ctx: &'a Ctx,
    r: &'a dyn Runner,
    /// The bind address a POST's `Host` must name; None: read-only.
    addr: Option<String>,
    /// Command stdout by argv, with its time: rate-limited reads, shared with the watcher, which (`fresh`) re-reads at a third of the ttl.
    cache: Cache,
    fresh: bool,
    /// The background sampler's last host read (`serve_on`); a request only reads it.
    pub host: Arc<Mutex<Host>>,
    /// Reply-box send times per device hash (10 a minute).
    sent: Mutex<BTreeMap<String, Vec<u64>>>,
    /// `/live` client threads (at most 8; the paired device's hash, shared with the watcher), the
    /// watcher's state, and `kick`: an answer was tapped, the watcher re-reads panes and pushes now.
    clients: Clients,
    kick: Arc<AtomicBool>,
    watch: Mutex<Watch>,
    /// `serve_on`'s background queue for ops that can spawn or merge, and the fleets with one in flight.
    jobs: Option<Sender<Job>>,
    running: Mutex<BTreeSet<String>>,
    /// `serve --export` (#39): Some(demo), static files with relative URLs; `true` is the inert demo.
    export: Option<bool>,
}

const JSON: &str = "application/json; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";

/// A `303 See Other` to `loc`.
fn see(loc: &str) -> Vec<u8> {
    format!("HTTP/1.1 303 See Other\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
}

fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    head.lines().skip(1).find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
    })
}

/// #199: the asking client's address. Behind `tailscale serve` every peer is loopback: then the forwarded address (the
/// proxy appends it last) or the tailnet login; those headers count from a loopback peer only.
fn client(head: &str) -> &str {
    let peer = header(head, "x-peer").unwrap_or_default();
    let fwd = header(head, "x-forwarded-for")
        .and_then(|f| f.rsplit(',').next())
        .map(str::trim);
    let fwd = fwd
        .or_else(|| header(head, "tailscale-user-login"))
        .filter(|f| !f.is_empty());
    peer.parse::<std::net::IpAddr>()
        .is_ok_and(|p| p.is_loopback())
        .then_some(fwd)
        .flatten()
        .unwrap_or(peer)
}

/// `application/x-www-form-urlencoded` (also the query string) into a map.
fn form(s: &str) -> BTreeMap<String, String> {
    let dec = |v: &str| {
        let (b, mut out, mut i) = (v.as_bytes(), vec![], 0);
        while i < b.len() {
            match (b[i], v.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())) {
                (b'%', Some(x)) => (out.push(x), i += 2).1,
                (b'+', _) => out.push(b' '),
                (c, _) => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    s.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (dec(k), dec(v)))
        .collect()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// #166, #162 A: a fleet card's second line: `⟳ driving…`, else the last pass's age and result in its colour
/// (`drive::result`); a tap lists `meta` (stall check, lanes, serial), then each action
/// in plain words, a tap on one its raw code.
fn drive_line(ctx: &Ctx, events: &[Event], fleet: &str, meta: &[String]) -> String {
    let pass = events.iter().rev().find(|e| e.kind == "drive-pass" && e.fleet == fleet);
    let (tone, line) = match pass {
        _ if crate::drive::running(ctx, fleet) => ("m", "⟳ driving…".to_string()),
        None => ("a", "drive · no pass yet".to_string()),
        Some(e) => {
            let (tone, line) = crate::drive::result(e);
            let rest = line.split_once(" · ").map_or("", |r| r.1).to_string();
            (
                tone,
                format!("drive · {} ago · {rest}", age(ctx.now.saturating_sub(e.ts))),
            )
        }
    };
    let acts = pass
        .and_then(|e| e.data.get("actions").and_then(serde_json::Value::as_array).cloned())
        .unwrap_or_default();
    let items: String = acts
        .iter()
        .map(|a| {
            let who = a["issue"]
                .as_u64()
                .map_or_else(|| a["step"].as_str().unwrap_or_default().into(), |n| format!("#{n}"));
            let (w, code) = (
                a["words"].as_str().unwrap_or_default(),
                a["code"].as_str().unwrap_or("ok"),
            );
            format!(
                "<li><details><summary>{}: {}</summary><code>{code}</code></details></li>",
                esc(&who),
                esc(w)
            )
        })
        .collect();
    let items = if items.is_empty() && pass.is_some() {
        "<li>nothing was due</li>".into()
    } else {
        items
    };
    let meta: String = meta.iter().map(|m| format!("<li>{}</li>", esc(m))).collect();
    format!(
        "<details class=\"dp {tone}\"><summary><i>●</i><span>{}</span><i class=cv></i></summary><ul>{meta}{items}</ul></details>",
        esc(&line)
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// fleet.toml's mtime (ns), rendered into every form: a POST refuses STALE when it moved.
fn mtime(ctx: &Ctx) -> String {
    let t = std::fs::metadata(&ctx.config).and_then(|m| m.modified()).ok();
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(String::new(), |d| d.as_nanos().to_string())
}

/// An error as the page shows it: `REFUSED <CODE>: …` or `error: …`.
fn shown(e: &anyhow::Error) -> String {
    e.downcast_ref::<Refusal>()
        .map_or_else(|| format!("error: {e:#}"), std::string::ToString::to_string)
}

/// `v` is `name`, or `name:<port>`.
fn names(name: &str, v: &str) -> bool {
    v == name
        || v.rsplit_once(':')
            .is_some_and(|(n, p)| n == name && p.chars().all(|c| c.is_ascii_digit()))
}

/// green = merged/PASS/ready/working, amber = queued/impl/review/fix/idle, red = blocked/dead/FAIL/needs you.
fn tone(state: &str) -> &'static str {
    match state.split('(').next().unwrap_or_default() {
        "merged" | "ready" | "PASS" | "working" | "done" | "ok" | "normal" => "g",
        "blocked" | "needs-human" | "⏸ waiting: decision" | "dead" | "FAIL" | "critical" | "capped" => "r",
        "parked" => "m",
        _ => "a",
    }
}

/// The non-empty HTML lines, one `<p>` each.
fn card(lines: &[String]) -> String {
    lines
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| format!("<p>{l}</p>"))
        .collect()
}

/// One narrow 3-field row (`a`, state, age) that expands to the stacked card `lines`.
fn row(class: &str, lead: &str, a: &str, state: &str, age: &str, lines: &[String]) -> String {
    let card = card(lines);
    format!(
        "<details class=\"{class}\"><summary>{lead}<span>{}</span><span class={}>{}</span><span class=m>{}</span></summary>{card}</details>",
        esc(a),
        tone(state),
        esc(state),
        esc(age)
    )
}

/// Form ops only a paired device may POST.
const PAIRED_OPS: &[&str] = &[
    "unpair", "answer", "drive", "override", "extend", "park", "orphans", "reap",
];

/// The non-empty fields, ` · `-joined (no `-` placeholders).
fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Ask `a`'s answer form: option buttons (recommended first) and a free-text box.
fn answer_form(ctx: &Ctx, a: &Event, send: &str) -> String {
    const REC: &str = " <small>(recommended)</small>";
    let rec = a.data["recommend"].as_u64();
    let btn = |(o, n): (&serde_json::Value, u64)| {
        let o = esc(o.as_str().unwrap_or("?"));
        let mark = if Some(n) == rec { REC } else { "" };
        format!("<button name=choice value={n}>{n}. {o}{mark}</button>")
    };
    let mut opts: Vec<_> = a.data["options"].as_array().into_iter().flatten().zip(1..).collect();
    opts.sort_by_key(|o| Some(o.1) != rec);
    let btns: String = opts.into_iter().map(btn).collect();
    let id = esc(a.str("id").unwrap_or_default());
    let op = hidden("answer", &a.fleet, &mtime(ctx));
    format!("<form method=post>{op}<input type=hidden name=id value=\"{id}\"><input name=text aria-label=answer placeholder=\"or type an answer\"><button>{send}</button>{btns}</form>")
}

fn hidden(op: &str, fleet: &str, mt: &str) -> String {
    format!("<input type=hidden name=op value={op}><input type=hidden name=fleet value=\"{}\"><input type=hidden name=mtime value={mt}>", esc(fleet))
}

const GALLOP: &[&str] = &["    _/>\n~(___)\n /  \\", "    _/>\n~(___)\n  ><"];
const STAND: &str = "    _/>\n~(___)\n  | |";
const REST: &str = "\n    _/>\n~(___)_";
const REAR: &str = "     />\n ~(_/\n  / \\";

/// Corral's 8 coats (HerdArt.swift): body, shade, mane; chestnut, palomino, dun and buckskin carry a blaze.
const COATS: &[(&str, &str, &str, bool)] = &[
    ("8a5a33", "6f4527", "2e2019", false),
    ("a05c2c", "7d4520", "5d3317", true),
    ("3b3b44", "2a2a31", "17161a", false),
    ("b9bcc6", "989ca9", "7d8089", false),
    ("c89a56", "a37a3c", "e8dcc0", true),
    ("b08d5e", "8e6f45", "41321f", true),
    ("96685a", "77524a", "4a342c", false),
    ("bd8f4e", "96703a", "241a10", true),
];

/// The coat as CSS vars, stable per name.
fn coat(name: &str) -> String {
    let (c, d, m, blaze) = COATS[usize::from_str_radix(&devices::sha256(name.as_bytes())[..2], 16).unwrap_or(0) % 8];
    format!(
        "--c:#{c};--d:#{d};--m:#{m};--b:{}",
        if blaze { "#efe7dc" } else { "transparent" }
    )
}

/// `age` since `t`, or nothing when `t` is unknown (0).
fn ago(now: u64, t: u64) -> String {
    if t > 0 {
        age(now.saturating_sub(t))
    } else {
        String::new()
    }
}

/// A lane as the Lanes row, its sheet card and herd.json show it: (cached title, tone, shown state, age).
/// #46: an idle lane shows what it waits on, coloured by who; a working one keeps its state.
type View = (Option<String>, &'static str, String, String);
fn view(ctx: &Ctx, l: &lanes::Lane, waits: &[crate::waiting::Waiting]) -> View {
    let title = l
        .issue
        .zip(l.fleet.as_ref())
        .and_then(|(n, f)| gh::cached_title(ctx, f, n));
    let (tone, shown) = wait_of(l, waits).map_or_else(
        || (tone(&l.state), l.state.clone()),
        |w| (w.tone(ctx.now), w.label(ctx.now)),
    );
    let pr = l
        .issue
        .zip(l.fleet.as_ref())
        .map(|(n, f)| gh::cached_pr(ctx, f, n))
        .filter(|p| p["state"] == "OPEN");
    let on = |h: &str| {
        pr.as_ref().map_or(format!("pushed {}", h.trim()), |p| {
            format!("work on PR #{}", p["number"])
        })
    };
    let (tone, shown) = l
        .parked
        .as_ref()
        .map_or((tone, shown), |h| ("m", format!("parked · {}", on(h))));
    (title, tone, shown, ago(ctx.now, l.since))
}

/// The lane's wait reason while herdr shows it idle, done or unknown.
fn wait_of(l: &lanes::Lane, waits: &[crate::waiting::Waiting]) -> Option<crate::waiting::Waiting> {
    let w = waits.iter().find(|w| w.lane == l.name).cloned();
    w.filter(|_| matches!(l.state.as_str(), "idle" | "done" | "unknown" | "parked"))
}

/// The state as an avatar in all three modes (CSS shows `body[data-mode]`'s); coat stable per name.
pub fn avatar(name: &str, state: &str) -> String {
    let (pose, glyph, art) = match state {
        "working" => (
            1,
            "▶",
            format!("<i class=\"asc run\"><b>{}</b><b>{}</b></i>", GALLOP[0], GALLOP[1]),
        ),
        "done" => (3, "✓", format!("<i class=asc><b>{REST}</b></i>")),
        "blocked" | "dead" => (4, "✗", format!("<i class=asc><b>{REAR}</b></i>")),
        _ => (2, "⏸", format!("<i class=asc><b>{STAND}</b></i>")),
    };
    format!("<span class=\"av {} {state}\"><i class=gl>{glyph}</i>{art}<svg class=px viewBox=\"0 0 132 100\" style=\"{}\"><use href=#hp{pose} /></svg></span>", tone(state), coat(name))
}

/// The device kind and browser from `User-Agent` ("iPhone Safari"), for the Host tab's device lists.
fn kind(head: &str) -> String {
    let ua = header(head, "user-agent").unwrap_or_default();
    let k = |l: &[&'static str], d| l.iter().copied().find(|k| ua.contains(k)).unwrap_or(d);
    format!(
        "{} {}",
        k(&["iPhone", "iPad", "Android", "Mac", "Windows", "Linux"], "device"),
        k(&["Firefox", "Edg", "Chrome", "Safari"], "")
    )
    .trim_end()
    .into()
}

impl<'a> Server<'a> {
    pub fn new(ctx: &'a Ctx, r: &'a dyn Runner, addr: Option<String>) -> Self {
        Server {
            ctx,
            r,
            addr,
            cache: Arc::default(),
            fresh: false,
            host: Arc::default(),
            sent: Mutex::default(),
            clients: Arc::default(),
            kick: Arc::default(),
            watch: Mutex::default(),
            jobs: None,
            running: Mutex::default(),
            export: None,
        }
    }

    fn handle(&self, mut s: TcpStream) -> std::io::Result<()> {
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let (mut buf, mut chunk) = (vec![], [0u8; 2048]);
        let done = |b: &[u8]| {
            let t = String::from_utf8_lossy(b);
            t.split_once("\r\n\r\n").is_some_and(|(h, body)| {
                body.len() >= header(h, "content-length").and_then(|n| n.parse().ok()).unwrap_or(0)
            })
        };
        while !done(&buf) && buf.len() < 32_768 {
            match s.read(&mut chunk)? {
                0 => break,
                n => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let req = String::from_utf8_lossy(&buf).replacen(
            "\r\n",
            &format!(
                "\r\nX-Peer: {}\r\n",
                s.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default()
            ),
            1,
        );
        let mut clients = lock(&self.clients);
        clients.retain(|c| c.0.send(String::new()).is_ok());
        if req.starts_with("GET /live ") && self.allowed(self.ctx, &req) && clients.len() < 8 {
            s.set_write_timeout(Some(Duration::from_secs(10)))?;
            s.set_read_timeout(Some(Duration::from_millis(1)))?;
            let (tx, rx) = channel::<String>();
            clients.push((
                tx,
                self.device(
                    &Ctx {
                        now: crate::now(),
                        ..self.ctx.clone()
                    },
                    &req,
                )
                .map(|d| d.hash),
            ));
            let mut msg =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n: ping\n\n"
                    .to_string();
            // Each second: a frag, else a peek (the peer closed: free the slot), else a ping every 15 s.
            std::thread::spawn(move || {
                while s.write_all(msg.as_bytes()).is_ok() {
                    let t = Instant::now();
                    msg = loop {
                        match rx.recv_timeout(Duration::from_secs(1)) {
                            Ok(m) if !m.is_empty() => break m,
                            Err(RecvTimeoutError::Disconnected) => return,
                            _ if s
                                .peek(&mut [0])
                                .map_or_else(|e| !matches!(e.kind(), WouldBlock | TimedOut), |n| n == 0) =>
                            {
                                return
                            }
                            _ if t.elapsed() >= Duration::from_secs(15) => break ": ping\n\n".into(),
                            _ => {}
                        }
                    };
                }
            });
            return Ok(());
        }
        drop(clients);
        s.write_all(&self.respond(&req, crate::now()))
    }

    /// The watcher's 1 s tick (docs/serve.md): on a change, re-render per audience and push only the changed fragments.
    fn tick(&self) {
        let mut w = lock(&self.watch);
        let now = crate::now();
        let known: Vec<String> = devices::load(self.ctx).into_iter().map(|d| d.hash).collect();
        lock(&self.clients).retain(|c| c.1.as_ref().is_none_or(|h| known.contains(h)));
        let log = std::fs::metadata(&self.ctx.state)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        let kicked = self.kick.swap(false, Relaxed);
        if kicked {
            self.purge();
        }
        let driving = self.ctx.state.with_file_name("drive.lock").exists();
        let mut changed = kicked || log != w.log || driving != w.driving || now >= w.rendered + 15;
        (w.log, w.driving) = (log, driving);
        if now >= w.herdr.0 + 2 {
            let out = herdr::agent_list(self.r);
            changed |= hash(&out) != w.herdr.1;
            w.herdr = (now, hash(&out));
            lock(&self.cache).insert("herdr agent list".into(), (now, out));
        }
        // #37: Herd's data rides the same stream, re-read every 10 s (host weather) while anyone listens;
        // like a frag, pushed only on a change (the page reads `/herd.json` when Herd opens).
        if (changed || now >= w.herd.0 + 10) && !lock(&self.clients).is_empty() {
            let j = self
                .herd(
                    &Ctx {
                        now,
                        ..self.ctx.clone()
                    },
                    None,
                )
                .map(|v| v.to_string())
                .unwrap_or_default();
            if w.herd.0 > 0 && hash(&j) != w.herd.1 && !j.is_empty() {
                lock(&self.clients).retain(|c| c.0.send(format!("event: herd\ndata: {j}\n\n")).is_ok());
            }
            w.herd = (now, hash(&j));
        }
        if !changed {
            return;
        }
        w.rendered = now;
        for paired in [false, true] {
            for (id, html) in self
                .frags(
                    &Ctx {
                        now,
                        ..self.ctx.clone()
                    },
                    false,
                    paired,
                )
                .unwrap_or_default()
            {
                if w.frags
                    .insert((paired, id), hash(&html))
                    .is_some_and(|h| h != hash(&html))
                {
                    let msg = format!("event: frag\ndata: {}\n\n", serde_json::json!({"id": id, "html": html}));
                    lock(&self.clients).retain(|c| c.1.is_some() != paired || c.0.send(msg.clone()).is_ok());
                }
            }
        }
    }

    /// Forget cached pane reads: the next render reads each pane afresh.
    fn purge(&self) {
        lock(&self.cache).retain(|k, _| !k.starts_with("herdr pane read"));
    }

    /// The runner whose reads that ran OK within the last `ttl` seconds come from the cache (else run now).
    fn cached(&self, now: u64, ttl: u64) -> Memo<'_> {
        Memo(self.r, &self.cache, now, if self.fresh { ttl / 3 } else { ttl })
    }

    /// The full HTTP response to one request.
    pub fn respond(&self, req: &str, now: u64) -> Vec<u8> {
        let (head, body) = req.split_once("\r\n\r\n").unwrap_or((req, ""));
        let mut w = head.split_whitespace();
        let (method, target) = (w.next().unwrap_or_default(), w.next().unwrap_or_default());
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let ctx = &Ctx {
            now,
            ..self.ctx.clone()
        };
        let text = |s: &str| s.as_bytes().to_vec();
        let fail = |e: anyhow::Error| {
            (
                "500 Internal Server Error",
                "text/plain",
                format!("{e:#}\n").into_bytes(),
            )
        };
        let (mut extra, mut cache) = (String::new(), "no-store");
        // #67: a fetch-only read never becomes the page; `fetch()` sends neither header.
        let nav = header(head, "sec-fetch-mode") == Some("navigate")
            || header(head, "sec-fetch-dest") == Some("document")
            || header(head, "accept").is_some_and(|a| a.starts_with("text/html"));
        let frags = path == "/frags" || path == "/" && query == "frag=all"; // `?frag=all`: old cached JS, one release
        if method == "GET" && nav && (frags || ["/herd.json", "/lane", "/frag"].contains(&path)) {
            return see("/");
        }
        // Host/Origin-checked: the live stream, transcripts, Herd's data and every write.
        let guarded = method == "GET" && ["/live", "/lane", "/herd.json"].contains(&path)
            || method == "POST" && self.addr.is_some();
        let paired_op = form(body.trim())
            .get("op")
            .is_some_and(|o| PAIRED_OPS.contains(&o.as_str()));
        let (status, ctype, body) = match (method, path) {
            ("GET", _) if frags => self.frags(ctx, false, self.device(ctx, head).is_some()).map_or_else(fail, |f| {
                ("200 OK", JSON, serde_json::to_vec(&f.into_iter().collect::<BTreeMap<_, _>>()).unwrap_or_default())
            }),
            ("POST", _) if self.addr.is_none() => ("405 Method Not Allowed", "text/plain", text("read-only: GET only\n")),
            _ if guarded && !self.allowed(ctx, head) => ("403 Forbidden", "text/plain", text("forbidden: foreign Host or Origin\n")),
            ("GET", "/live") => ("503 Service Unavailable", "text/plain", text("8 live clients already: the page polls instead\n")),
            ("GET", "/") => self.page(ctx, &form(query), self.device(ctx, head).is_some())
                .map_or_else(fail, |b| ("200 OK", HTML, b.into_bytes())),
            ("GET", "/frag") => self.part(ctx, &form(query))
                .map_or(("404 Not Found", "text/plain", text("no such part\n")), |h| ("200 OK", HTML, h.into_bytes())),
            ("GET", "/lane") if self.device(ctx, head).is_none() => ("401 Unauthorized", "text/plain", text("pair this device to read transcripts\n")),
            ("GET", "/lane") => self.transcript(ctx, &form(query)).map_or(
                ("404 Not Found", "text/plain", text("no such agent pane\n")),
                |j| ("200 OK", JSON, j.into_bytes()),
            ),
            ("GET", "/herd.json") => {
                let loopback = header(head, "host").is_some_and(|h| ["127.0.0.1", "localhost", "[::1]"].iter().any(|l| names(l, h)));
                let weather = form(query).get("weather").filter(|_| loopback).cloned();
                self.herd(ctx, weather.as_deref()).map_or_else(fail, |j| ("200 OK", JSON, j.to_string().into_bytes()))
            }
            ("GET", p) if p.starts_with("/theme/") => match THEME.iter().find(|f| p == format!("/theme/{f}")) {
                Some(f) => theme(ctx, f).0.map_or(("404 Not Found", "text/plain", text("not in this theme\n")), |b| ("200 OK", if f.ends_with("json") { JSON } else { SVG }, b)),
                None => ("404 Not Found", "text/plain", text("not a theme file\n")),
            },
            ("GET", "/sw.js") => ("200 OK", "text/javascript", text(&SW.replace("@V", &format!("{VER:08x}")))),
            ("GET", p) => {
                let a = ASSETS.iter().find(|a| p == format!("/a/{VER:08x}/{}", a.0)).inspect(|_| cache = "public, max-age=31536000, immutable");
                a.or_else(|| FILES.iter().find(|f| p.strip_prefix('/') == Some(f.0)).inspect(|_| cache = "public, max-age=86400")).map_or(("404 Not Found", "text/plain", text("not found\n")), |f| ("200 OK", f.1, f.2.to_vec()))
            }
            ("POST", "/") if paired_op && self.device(ctx, head).is_none() => {
                ("401 Unauthorized", "text/plain", text("only a paired device may unpair, answer or drive\n"))
            }
            ("POST", "/") => {
                // #140: an op that can spawn or merge runs on `serve_on`'s job thread; the reply is at once.
                let f = form(body.trim());
                let [op, fleet] = ["op", "fleet"].map(|k| f.get(k).cloned().unwrap_or_default());
                let msg = match self.jobs.as_ref().filter(|_| ["drive", "override", "extend"].contains(&op.as_str())) {
                    Some(_) if !lock(&self.running).insert(fleet.clone()) => format!("REFUSED DRIVE_RUNNING: {fleet} has a {op} running"),
                    Some(tx) => (format!("{op} started for {fleet}"), drop(tx.send((ctx.clone(), f, fleet)))).0,
                    None => self.write(ctx, &f).unwrap_or_else(|e| shown(&e)),
                };
                return see(&format!("/?msg={}", enc(&msg)));
            }
            // #59: the request's `hd=` token (10 min) works once `serve approve` clears its code; `/pair/claim` then keeps it a year.
            // #199: a re-tap replaces this browser's own open request, so repeated taps never lock it out.
            ("POST", "/pair/request") => match devices::cancel(ctx, header(head, "cookie").unwrap_or_default())
                .and_then(|()| devices::request(ctx, &kind(head), client(head)))
            {
                Ok(Some((code, t))) => {
                    extra = format!("Set-Cookie: hd={t}; Path=/; Max-Age=600; HttpOnly; Secure; SameSite=Strict\r\n");
                    let hint = Config::load(&ctx.config).ok().and_then(|c| c.serve.approve_hint).unwrap_or_default();
                    ("200 OK", "text/plain", format!("{code}\n{}\n", hint.replace("{code}", &code)).into_bytes())
                }
                Ok(None) => ("429 Too Many Requests", "text/plain", text("4 pairing requests open (2 per address), or a code clash: approve one, tap Pair again or wait 10 min\n")),
                Err(e) => fail(e),
            },
            ("POST", "/pair/cancel") => match devices::cancel(ctx, header(head, "cookie").unwrap_or_default()) {
                Ok(()) => ("200 OK", "text/plain", text("pairing request cancelled\n")),
                Err(e) => fail(e),
            },
            ("POST", "/pair/claim") => match self.device(ctx, head) {
                Some(d) => (extra = format!("Set-Cookie: {}; Path=/; Max-Age=31536000; HttpOnly; Secure; SameSite=Strict\r\n", header(head, "cookie").unwrap_or_default().split(';').map(str::trim).find(|c| c.starts_with("hd=")).unwrap_or_default()), ("200 OK", "text/plain", format!("paired as {}\n", d.label).into_bytes())).1,
                None => ("403 Forbidden", "text/plain", text("not approved yet: `herdsman serve approve <code>` on the Mac\n")),
            },
            ("POST", "/push/subscribe" | "/push/unsubscribe") if self.device(ctx, head).is_none() => ("401 Unauthorized", "text/plain", text("pair this device first\n")),
            ("POST", "/push/subscribe") => {
                let sub = Config::load(&ctx.config).ok().and_then(|c| c.webpush_sub().map(String::from));
                match crate::webpush::subscribe(&ctx.state, &form(body.trim()), &kind(head), now, sub.as_deref()) {
                    Ok(msg) => ("200 OK", "text/plain", format!("{msg}\n").into_bytes()),
                    Err(e) => ("400 Bad Request", "text/plain", format!("{}\n", shown(&e)).into_bytes()),
                }
            }
            ("POST", "/push/unsubscribe") => {
                let id = form(body.trim()).get("id").cloned().unwrap_or_default();
                let msg = crate::webpush::remove(&ctx.state, &[id]).map_or_else(|e| shown(&e), |n| format!("{n} alert device removed"));
                return see(&format!("/?msg={}", enc(&msg)));
            }
            ("POST", p @ ("/send" | "/answer" | "/act")) => {
                let (status, msg) = self.send(ctx, head, &form(body.trim()), p);
                (status, "text/plain", format!("{msg}\n").into_bytes())
            }
            ("POST", _) => ("404 Not Found", "text/plain", text("not found\n")),
            _ => ("405 Method Not Allowed", "text/plain", text("GET or POST only\n")),
        };
        let allow = if self.addr.is_some() { "GET, POST" } else { "GET" };
        let mut res = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAllow: {allow}\r\n{extra}Cache-Control: {cache}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        res.extend(body);
        res
    }

    /// `Host` names the bind address or a `[serve].allowed_hosts` name, and any `Origin` is one of those.
    fn allowed(&self, ctx: &Ctx, head: &str) -> bool {
        let mut hosts = vec![self.addr.clone().unwrap_or_default()];
        hosts.extend(
            Config::load(&ctx.config)
                .map(|c| c.serve.allowed_hosts)
                .unwrap_or_default(),
        );
        let host = header(head, "host").unwrap_or_default();
        let origin =
            header(head, "origin").map(|o| o.strip_prefix("https://").or(o.strip_prefix("http://")).unwrap_or("\0"));
        hosts.iter().any(|h| names(h, host)) && origin.is_none_or(|o| hosts.iter().any(|h| names(h, o)))
    }

    /// The Pair this phone button (#59), on the live, writable page only.
    fn pairb(&self) -> &'static str {
        if self.addr.is_some() && self.export.is_none() {
            " <button type=button class=pairb>Pair this phone</button>"
        } else {
            ""
        }
    }

    /// The paired device whose token the request's `Cookie` carries.
    #[allow(clippy::unused_self)] // a method so every route reads `self.device(..)` alike
    fn device(&self, ctx: &Ctx, head: &str) -> Option<devices::Device> {
        devices::find(ctx, header(head, "cookie").unwrap_or_default())
    }

    /// The reply box (or a blocked menu's option, or a sheet action): a paired device (cookie) prompts one agent, at most 10 sends a minute.
    fn send(&self, ctx: &Ctx, head: &str, f: &BTreeMap<String, String>, path: &str) -> (&'static str, String) {
        let Some(dev) = self.device(ctx, head) else {
            return ("401 Unauthorized", "pair this device: tap Pair this phone".into());
        };
        let mut sent = lock(&self.sent);
        let times = sent.entry(dev.hash.clone()).or_default();
        times.retain(|t| ctx.now < t + 60);
        if times.len() >= 10 {
            return ("429 Too Many Requests", "10 sends a minute: wait a moment".into());
        }
        times.push(ctx.now);
        let (pick, act) = (path == "/answer", path == "/act");
        let res = if pick {
            self.pick(ctx, &dev, f)
        } else if act {
            self.act(ctx, &dev, f)
        } else {
            self.prompt(ctx, &dev, f)
        };
        if pick || act {
            self.purge();
            self.kick.store(true, Relaxed);
        }
        res.map_or_else(|e| ("409 Conflict", shown(&e)), |msg| ("200 OK", msg))
    }

    /// The live fleet lane `agent` with its fleet and pane; refused PAUSED when its fleet is.
    fn lane(&self, ctx: &Ctx, agent: &str) -> Result<(lanes::Lane, String, String)> {
        let (cfg, events) = (Config::load(&ctx.config)?, read(&ctx.state)?);
        let all = self
            .lanes(ctx.now, &cfg, &events, &fold(&events))
            .context("herdr agent list failed")?;
        let l = all
            .into_iter()
            .find(|l| l.name == agent)
            .with_context(|| format!("no agent `{agent}`"))?;
        let (Some(fleet), Some(pane)) = (l.fleet.clone(), l.pane.clone()) else {
            bail!("`{agent}` is not a live fleet lane")
        };
        if cfg.fleet.get(&fleet).is_some_and(|f| f.paused) {
            return refused("PAUSED", format!("{fleet} is paused (`herdsman resume {fleet}`)"));
        }
        Ok((l, fleet, pane))
    }

    /// A blocked Claude menu's option: its digit via `pane send-keys`, then a re-read ("sent" or
    /// "still waiting"); logs an `answer` event without the question. No menu parsed, or not the
    /// menu the tapped card showed (`h`, read afresh): no key.
    fn pick(&self, ctx: &Ctx, dev: &devices::Device, f: &BTreeMap<String, String>) -> Result<String> {
        let (agent, choice) = (
            f.get("agent").map_or("", String::as_str),
            f.get("choice").map_or("", String::as_str),
        );
        let (l, fleet, pane) = self.lane(ctx, agent)?;
        let screen = || herdr::read(self.r, &pane);
        let Some((q, opts)) = lanes::pending(&screen()?).filter(|p| l.harness == "claude" && !p.1.is_empty()) else {
            return refused(
                "BAD_VALUE",
                format!("no menu on {agent}'s pane: menu changed — refreshed; answer with the reply box"),
            );
        };
        if reviewer(ctx, &fleet, &l.role) && !q.starts_with("Do you want") {
            return refused("REV_READONLY", "reviewer lanes answer permission prompts only");
        }
        if !choice
            .parse()
            .is_ok_and(|n: usize| (1..=opts.len().min(9)).contains(&n))
            || choice.len() != 1
        {
            return refused("BAD_VALUE", format!("choose 1 to {}", opts.len()));
        }
        if f.get("h") != Some(&seen(&q, &opts).to_string()) {
            return refused("STALE", "menu changed — refreshed");
        }
        herdr::keys(self.r, &pane, choice)?;
        let d = serde_json::json!({"via": "web", "agent": agent, "device": dev.label, "choice": choice});
        append(&ctx.state, &Event::new(ctx.now, &fleet, l.issue, "answer", d))?;
        self.r.sleep(1);
        Ok(if lanes::pending(&screen()?) == Some((q, opts)) {
            format!("still waiting: {agent}")
        } else {
            format!("sent to {agent}")
        })
    }

    /// `herdr agent prompt` (dsh: `pane send-text`) + Enter; logs a `prompt` event without the text.
    fn prompt(&self, ctx: &Ctx, dev: &devices::Device, f: &BTreeMap<String, String>) -> Result<String> {
        let g = |k: &str| f.get(k).map_or("", String::as_str);
        let (agent, text) = (g("agent"), g("text"));
        let (l, fleet, pane) = self.lane(ctx, agent)?;
        let (fleet, pane) = (&fleet, &pane);
        if reviewer(ctx, fleet, &l.role) {
            return refused("REV_READONLY", "reviewer lanes are not steered mid-review");
        }
        let n = text.chars().count();
        if text.trim().is_empty() || n > 2000 {
            return refused("BAD_VALUE", format!("a message is 1 to 2000 chars, not {n}"));
        }
        if text.chars().any(|c| (c < ' ' && c != '\n') || c == '\x7f') {
            return refused("BAD_VALUE", "a message has no control characters (newlines are fine)");
        }
        herdr::reply(self.r, l.harness == "dsh", agent, pane, text)?;
        let d = serde_json::json!({"via": "web", "agent": agent, "device": dev.label, "chars": n});
        append(&ctx.state, &Event::new(ctx.now, fleet, l.issue, "prompt", d))?;
        Ok(format!("sent to {agent}"))
    }

    /// A sheet action (`herdsman stop|kill|clean --lane`, or `close <pane>` for an Others agent; kill and clean dry-run without `confirm=1`); logs `act` unless a dry run.
    fn act(&self, ctx: &Ctx, dev: &devices::Device, f: &BTreeMap<String, String>) -> Result<String> {
        let (g, mut out, cfg) = (
            |k: &str| f.get(k).map_or("", String::as_str),
            vec![],
            Config::load(&ctx.config)?,
        );
        let (agent, yes) = (g("agent"), g("confirm") == "1");
        let sp = crate::events::last_spawn(&read(&ctx.state)?, agent).map(|e| (e.fleet.clone(), e.issue));
        let Some((fleet, issue)) = sp.or_else(|| (g("op") == "close").then(|| (String::new(), None))) else {
            return refused("NO_SUCH_AGENT", format!("no lane `{agent}` in the log"));
        };
        match g("op") {
            "stop" => crate::check::stop(ctx, &cfg, self.r, &mut out, agent)?,
            "kill" => crate::check::kill(ctx, &cfg, self.r, &mut out, agent, yes)?,
            "clean" => crate::check::clean(ctx, &cfg, self.r, &mut out, Some(&fleet), (None, Some(agent)), yes)?,
            "close" => crate::check::close(ctx, &cfg, self.r, &mut out, agent)?,
            op => return refused("BAD_VALUE", format!("op is stop, kill, clean or close, not `{op}`")),
        }
        if yes || g("op") == "stop" {
            let d = serde_json::json!({"via": "web", "agent": agent, "device": dev.label, "op": g("op")});
            append(&ctx.state, &Event::new(ctx.now, &fleet, issue, "act", d))?;
        }
        Ok(String::from_utf8_lossy(&out).trim().replace('\n', "; "))
    }

    /// The sheet's transcript: up to 200 masked ANSI lines `before` the newest (load more) of the lane's rolling
    /// transcript (#138; the export reads the screen only), the live block's length and the status line, as JSON.
    fn transcript(&self, ctx: &Ctx, q: &BTreeMap<String, String>) -> Option<String> {
        let n = |k: &str, d: usize| q.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        let (lines, before) = (n("lines", 200).min(200), n("before", 0));
        let (cfg, events) = (Config::load(&ctx.config).ok()?, read(&ctx.state).ok()?);
        let l = self
            .lanes(ctx.now, &cfg, &events, &fold(&events))?
            .into_iter()
            .find(|l| Some(&l.name) == q.get("agent"))?;
        let screen = herdr::recent(&self.cached(ctx.now, 5), l.pane.as_deref()?, "200").ok()?;
        let body = lanes::body(&screen);
        let file = lanes::log(&ctx.state, &l.name).filter(|_| self.export.is_none());
        let (all, live) = file.map_or((body.clone(), body.len()), |f| lanes::roll(&f, &body));
        let end = all.len().saturating_sub(before);
        let page = &all[end.saturating_sub(lines)..end];
        let status = lanes::chat(&screen).1.map(lanes::strip);
        Some(serde_json::json!({"harness": l.harness, "acts": if l.role == "other" { &["close"][..] } else { &["stop", "kill", "clean"] }, "pane": (l.role == "other").then_some(&l.pane), "lines": page, "live": if before == 0 { live.min(page.len()) } else { 0 }, "status": status, "before": before + page.len(), "more": end > page.len()}).to_string())
    }

    /// Run one form write; the line shown on the page (CLI output, or `REFUSED <CODE>: …` via `shown`).
    fn write(&self, ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<String> {
        let g = |k: &str| f.get(k).map_or("", String::as_str);
        if g("mtime") != mtime(ctx) {
            return refused(
                "STALE",
                "fleet.toml changed since the page was rendered; reload and retry",
            );
        }
        // #66: a short line for the toast, not the CLI's stdout (a queue move prints the whole queue).
        let (fleet, mut out) = (g("fleet"), vec![]);
        Ok(match g("op") {
            "mv" => {
                let n = |k: &str| g(k).parse::<u64>().ok();
                let issue = n("issue").context("no issue")?;
                let d = queue::op("mv", &[issue], false, n("before"), n("after"), None);
                crate::queue_change(ctx, &Config::load(&ctx.config)?, fleet, d, Some("web"))?;
                format!(
                    "moved #{issue} {}",
                    n("before").map_or_else(|| format!("after #{}", g("after")), |b| format!("before #{b}"))
                )
            }
            op @ ("pause" | "resume") => {
                crate::pause(ctx, &mut out, fleet, op == "pause", Some("web"))?;
                format!("{fleet} {op}d")
            }
            "set" => {
                let wanted: Vec<_> = ["harness", "model", "effort"]
                    .into_iter()
                    .filter(|k| !g(k).is_empty())
                    .map(|k| (k, g(k).to_string()))
                    .collect();
                let line = wanted
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                crate::set(ctx, &mut out, fleet, g("role"), wanted, Some("web"))?;
                format!("{fleet} {}: {line}", g("role"))
            }
            "answer" => {
                let choice = if g("choice").is_empty() { g("text") } else { g("choice") };
                crate::answer(
                    ctx,
                    &Config::load(&ctx.config)?,
                    self.r,
                    &mut out,
                    (fleet, g("id")),
                    choice,
                    ("owner", Some("web")),
                )?;
                format!("answered {}", g("id"))
            }
            "drive" => {
                let cfg = Config::load(&ctx.config)?;
                crate::drive::drive(ctx, &cfg, self.r, &mut out, Some(fleet), true, false)?;
                crate::drive::summary(fleet, &String::from_utf8_lossy(&out))
            }
            // #130: a needs-human card's buttons.
            op @ ("override" | "extend" | "park") => {
                let n = g("issue").parse().context("no issue")?;
                crate::gate::needs_human(
                    ctx,
                    &Config::load(&ctx.config)?,
                    self.r,
                    &mut out,
                    (fleet, n),
                    op,
                    (true, "owner"),
                )?;
                format!("#{n}: {op}")
            }
            "orphans" => {
                let n = crate::check::orphans(ctx, &Config::load(&ctx.config)?, &mut out, true)?;
                format!("removed {n} orphan folder(s)")
            }
            "reap" => {
                let cfg = Config::load(&ctx.config)?;
                let n = crate::check::leftovers(ctx, &cfg, self.r, &herdr::agents(self.r)?, true)?.len();
                format!("reaped {n} leftover process(es)")
            }
            "unpair" => {
                let mut all = devices::load(ctx);
                all.retain(|d| d.hash != g("hash"));
                devices::store(ctx, &all)?;
                "device removed".into()
            }
            op => bail!("unknown op `{op}`"),
        })
    }

    /// The live sections' inner HTML by element id (the page, `/?frag=all`, `/live`); pane text only when `paired`.
    fn frags(&self, ctx: &Ctx, all: bool, paired: bool) -> Result<Vec<(&'static str, String)>> {
        let events = read(&ctx.state)?;
        let (cfg, st) = (Config::load(&ctx.config).ok(), fold(&events));
        Ok(vec![
            ("needs", self.needs_you(ctx, cfg.as_ref(), &events, &st, paired)),
            ("queue", self.queue_tab(ctx, cfg.as_ref(), &events, &st)),
            (
                "lanes",
                "<p class=lh><button id=mode type=button aria-label=\"lane display mode\">horse</button></p>"
                    .to_string()
                    + &self.lanes_tab(ctx, cfg.as_ref(), &events, &st, paired),
            ),
            ("host", self.host_tab(ctx, cfg.as_ref(), &events)),
            ("events", events_tab(ctx, self.r, cfg.as_ref(), &events, all)),
        ])
    }

    /// #60: a part the page loads on demand: a fleet's [config] sheet (`id=config&fleet=`, #162), the Events tab (`id=events`).
    fn part(&self, ctx: &Ctx, q: &BTreeMap<String, String>) -> Option<String> {
        let (events, cfg) = (read(&ctx.state).ok()?, Config::load(&ctx.config).ok());
        match q.get("id")?.as_str() {
            "events" => Some(events_tab(ctx, self.r, cfg.as_ref(), &events, false)),
            "config" => {
                let (c, f) = (cfg?, q.get("fleet")?);
                Some(config_sheet(
                    ctx,
                    &c,
                    f,
                    c.fleet.get(f)?,
                    (self.addr.is_some(), &mtime(ctx)),
                    &fold(&events),
                ))
            }
            _ => None,
        }
    }

    fn page(&self, ctx: &Ctx, q: &BTreeMap<String, String>, paired: bool) -> Result<String> {
        let all = q.get("all").is_some_and(|v| v == "1");
        let mut f = self.frags(ctx, all, paired)?;
        // #60: the Events tab body loads when the tab opens (`/frag?id=events`), unless `?all=1` asked for it.
        if self.export.is_none() && !all {
            f[4].1.clear();
        }
        let msg = q.get("msg").map_or(String::new(), |m| {
            let bad = m.starts_with("REFUSED") || m.starts_with("error");
            format!("<p class=\"msg {}\">{}</p>", if bad { "r" } else { "g" }, esc(m))
        });
        let ro = if self.addr.is_some() { "" } else { " data-ro=1" };
        let (root, demo) = if self.export.is_some() {
            ("", self.export == Some(true))
        } else {
            ("/", false)
        };
        let ro = format!(
            "{ro}{}{}",
            if root.is_empty() { " data-root=\"\"" } else { "" },
            if demo { " data-demo=1" } else { "" }
        );
        // #199: iOS keeps Safari's and the home-screen app's cookies apart.
        let msg = if !paired && !self.pairb().is_empty() && devices::load(ctx).iter().any(|d| d.code.is_none()) {
            msg + "<p class=m>Safari and the home-screen app pair separately</p>"
        } else {
            msg
        };
        let msg = if demo {
            "<p class=\"msg a\">Demo with fake data. Install herdsman to run your own.</p>".to_string()
        } else {
            msg
        };
        let badge = if demo {
            "<span class=tag id=demo>demo</span>"
        } else {
            ""
        };
        Ok(format!(
            "<!doctype html><html lang=en><head><meta charset=utf-8>\
             <meta name=viewport content=\"width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover\">\
             <meta name=theme-color content=\"#1f2a38\"><meta name=apple-mobile-web-app-capable content=yes>\
             <link rel=manifest href={root}manifest.webmanifest><link rel=icon href={root}icon-192.png?v={VER:08x}><link rel=apple-touch-icon href={root}icon-180.png?v={VER:08x}>\
             <title>herdsman</title><link rel=stylesheet href={root}a/{VER:08x}/serve.css><link rel=stylesheet href={root}a/{VER:08x}/herd.css>\
             <script>let t=location.hash.slice(1);try{{t=t||localStorage.tab}}catch(e){{}}document.documentElement.dataset.tab=['lanes','host','events'].includes(t)?t:'queue'</script></head>\
             <body data-ts={} data-mode=horse{ro}><header><b>herdsman</b>{badge}<span id=stale hidden></span><i id=dot class=a></i>\
             <button type=button class=qhelp aria-label=\"what the marks mean\">?</button><button id=rf type=button aria-label=refresh>↻</button><button id=herd-on type=button aria-label=\"herd mode\">🐎</button>{}</header><i id=ptr hidden>↻</i>{msg}<div id=needs>{}</div><main>\
             <section id=queue>{}</section><section id=lanes>{}</section><section id=host>{}</section>\
             <section id=events>{}</section></main><nav><a href=#queue><span>queue</span></a><a href=#lanes><span>lanes</span></a>\
             <a href=#host><span>host</span></a><a href=#events><span>events</span></a></nav><script src={root}a/{VER:08x}/serve.js></script><script src={root}a/{VER:08x}/herd.js></script></body></html>\n",
            ctx.now, if paired { "" } else { self.pairb() }, f[0].1, f[1].1, f[2].1, f[3].1, f[4].1
        ))
    }

    /// Each open ask as a red card above every tab: options (recommended first) and a free-text box;
    /// then each blocked lane's pending prompt.
    fn needs_you(&self, ctx: &Ctx, cfg: Option<&Config>, events: &[Event], st: &States, paired: bool) -> String {
        let all = cfg.and_then(|c| self.lanes(ctx.now, c, events, st)).unwrap_or_default();
        let blocked: String = all
            .iter()
            .filter(|l| l.state == "blocked")
            .map(|l| self.blocked(ctx, l, paired))
            .collect();
        crate::events::open_asks(events).into_iter().map(|a| {
            let s = |k| a.str(k).unwrap_or_default();
            let who = join(&[&a.fleet, &a.issue.map_or(String::new(), |n| format!("#{n}")), s("agent"), &age(ctx.now.saturating_sub(a.ts))]);
            let mins = cfg.map_or(Limits::default().ask_default_minutes, |c| c.limits.ask_default_minutes);
            let rec = a.data["recommend"].as_u64().filter(|_| s("kind") == "routine");
            let left = age((a.ts + 60 * mins).saturating_sub(ctx.now));
            let kind = rec.map_or("owner · waits for you".into(), |n| format!("routine · defaults to {n} in {left}"));
            let who = esc(&who);
            let question = esc(s("question"));
            let form = self.addr.as_ref().map_or(String::new(), |_| answer_form(ctx, a, "Send"));
            format!("<div class=ask><p><b>needs you</b> <span class=m>{who}</span> <span class=m>· {kind}</span></p><p>{question}</p>{form}</div>")
        }).collect::<String>() + &blocked + &self.needs_human(ctx, cfg, events, st, paired)
    }

    /// #130: each `needs-human` issue's card: why, the last verdict, then (paired) merge anyway (PASS at head,
    /// CI green, mergeable), one more round, park.
    fn needs_human(&self, ctx: &Ctx, cfg: Option<&Config>, events: &[Event], st: &States, paired: bool) -> String {
        let (mt, rw) = (mtime(ctx), self.addr.is_some() && paired);
        let card = |s: &IssueState| {
            let (f, n) = (&s.fleet, s.issue);
            let (pr, fc) = (gh::cached_pr(ctx, f, n), cfg.and_then(|c| c.fleet.get(f)));
            let why = events.iter().rev().find(|e| e.is("needs-human", f, n));
            let why = esc(why.and_then(|e| e.str("reason")).unwrap_or("gate"));
            let m = pr["mergeable"].as_str().unwrap_or("?");
            let ok = fc.is_some_and(|c| gate(c, &pr, Some(s), true).is_ok()) && m == "MERGEABLE";
            let (line, m) = (pr_line(ctx, fc, f, n, None), m.to_lowercase());
            let fail = esc(crate::events::finding(events, f, n).unwrap_or_default());
            let v = match s.verdicts.last() {
                Some((sha, v, _)) if v == "PASS" => {
                    let stale = if pr["headRefOid"].as_str() == Some(sha.as_str()) {
                        ""
                    } else {
                        " (not the head)"
                    };
                    format!("PASS at {sha:.7}{stale} · {line} · {m}")
                }
                _ => format!("FAIL: {fail} · {line}"),
            };
            let merge = format!("<button name=op value=override data-confirm=\"Merge #{n} past the round limit?\">merge anyway</button>");
            let (merge, hid) = (if ok { merge.as_str() } else { "" }, hidden("", f, &mt));
            let form = format!("<form method=post>{hid}<input type=hidden name=issue value={n}>{merge}<button name=op value=extend>one more round</button><button name=op value=park>park</button></form>");
            let a = age(ctx.now.saturating_sub(s.last_ts));
            let title = esc(&gh::cached_title(ctx, f, n).unwrap_or_default());
            let (who, form) = (esc(&format!("{f} · #{n} · {a}")), if rw { form } else { String::new() });
            format!("<div class=ask data-n={n}><p><b>needs you</b> <span class=m>{who}</span></p><p>#{n} {title} · {why}</p><p class=m>{v}</p>{form}</div>")
        };
        st.values().filter(|s| s.state == "needs-human").map(card).collect()
    }

    /// A blocked lane's card: a Claude menu's options as buttons (`/answer`) under its title, reason, command
    /// and countdown (#127: `data-push=0` while that runs; none once the pane works again), else the question
    /// (or the last 6 masked lines) and a Reply button that opens the lane's sheet. Unpaired: no
    /// pane text, only the 1-line activity and a pairing hint.
    fn blocked(&self, ctx: &Ctx, l: &lanes::Lane, paired: bool) -> String {
        let screen = l
            .pane
            .as_ref()
            .and_then(|p| herdr::read(&self.cached(ctx.now, 2), p).ok())
            .unwrap_or_default();
        let rw = self.addr.is_some() && paired;
        let c = lanes::prompt(&screen).unwrap_or_default();
        let body = match lanes::pending(&screen).filter(|_| l.harness == "claude") {
            None if l.harness == "claude" && lanes::chat(&screen).1.is_some() => return String::new(),
            _ if !paired => format!(
                "<p class=m>{}</p><p class=m>pair this phone to answer{}</p>",
                esc(&lanes::activity(&l.harness, &screen)),
                self.pairb()
            ),
            Some((q, opts)) => {
                let s = |k: &str| format!("<p class={k}>{}</p>", esc(c[k].as_str().unwrap_or_default()));
                let cd = c["countdown"].as_u64().map(|n| (ctx.now + n, n / 60, n % 60));
                let cd = cd.map(|(u, m, s)| format!("<p class=cd data-until={u}>auto-denies in {m}:{s:02}</p>"));
                ["title", "reason", "command"].map(s).concat()
                    + &cd.unwrap_or_default()
                    + &format!("<p>{}</p>", esc(&lanes::mask(&q)))
                    + &opts
                        .iter()
                        .zip(1..)
                        .filter(|_| rw)
                        .map(|(o, n)| {
                            format!(
                                "<button type=button class=pick value={n} data-h={}>{n}. {}</button>",
                                seen(&q, &opts),
                                esc(&lanes::mask(o))
                            )
                        })
                        .collect::<String>()
            }
            None => format!("<pre>{}</pre>", esc(&lanes::tail(&screen, 6).join("\n"))),
        };
        let who = join(&[
            l.fleet.as_deref().unwrap_or_default(),
            &l.issue.map_or(String::new(), |n| format!("#{n}")),
            &l.name,
        ]);
        let reply = if rw {
            "<button type=button class=rp>Reply</button><p class=as></p>"
        } else {
            ""
        };
        format!(
            "<div class=ask data-ask=\"{}\" data-push={}><p><b>needs you</b> <span class=m>{}</span></p>{body}{reply}</div>",
            esc(&l.name),
            u8::from(c["push"].as_bool().unwrap_or(true)),
            esc(&who)
        )
    }

    /// `herdr agent|workspace list` JSON, read at most every 10 s (None when herdr cannot be read).
    fn herdr_json(&self, now: u64, what: &str) -> Option<serde_json::Value> {
        herdr::list(&self.cached(now, 10), what).ok()
    }

    /// Per configured fleet a card (#162): open work (needs you, ready, working, queued), `done today`; paused = one line.
    fn queue_tab(&self, ctx: &Ctx, cfg: Option<&Config>, events: &[Event], st: &States) -> String {
        let (now, mt, rw) = (ctx.now, mtime(ctx), self.addr.is_some());
        let Some(cfg) = cfg else {
            return "<p class=r>cannot read fleet.toml</p>".into();
        };
        // The state of an open lane herdr shows no pane for: `dead`, or `parked` (#83).
        let ls = self.lanes(now, cfg, events, st).unwrap_or_default();
        let gone = |s: &IssueState| {
            s.active_lane()
                .and_then(|x| ls.iter().find(|l| l.name == x.1 && l.pane.is_none()))
        };
        // #115: serve.js draws the rows as a graph from data-b/-q/-s; [?] (header) opens its legend, data-sp its serial line.
        // #162: each fleet a card in its colour; fleets that need you first, then by name.
        let mut cards: Vec<(bool, String)> = vec![];
        for (f, fc) in &cfg.fleet {
            let q = queue::view(events, st, f);
            let (sl, sp) = (esc(fc.serial_label()), esc(&fc.serial_paths.join(", ")));
            let sa = (!sp.is_empty()).then(|| format!(" data-s=\"{sl}\" data-sp=\"{sp}\""));
            let sa = sa.unwrap_or_default();
            let mine: Vec<&IssueState> = st.values().filter(|s| s.fleet == *f).collect();
            let queued = |n: u64| q.iter().any(|i| i.issue == n);
            let rest: Vec<u64> = mine
                .iter()
                .filter(|s| !queued(s.issue) && !s.done() && s.state != "queued")
                .map(|s| s.issue)
                .collect();
            let items: Vec<(u64, Option<u64>, Option<usize>)> = q
                .iter()
                .enumerate()
                .map(|(k, i)| (i.issue, i.blocked_by, Some(k)))
                .chain(rest.iter().map(|&n| (n, None, None)))
                .collect();
            let (mut rows, mut need) = (vec![], 0);
            for &(n, blocked_by, k) in &items {
                let s = st.get(&(f.clone(), n));
                let waiting = crate::events::open_asks(events)
                    .iter()
                    .any(|e| e.fleet == *f && e.issue == Some(n));
                let logged = s.map_or("queued", |s| s.state.as_str());
                let shown = if waiting { "⏸ waiting: decision" } else { logged };
                let state = s.and_then(gone).map_or(shown, |l| l.state.as_str());
                let is_dead = state == "dead";
                // #162: needs you, then ready, then working, then queued; serve.js keeps follow-ups under their parent.
                let rank = match state.split('(').next().unwrap_or_default() {
                    "needs-human" | "⏸ waiting: decision" => 0,
                    "ready" => 1,
                    "impl" | "fix" | "review" => 2,
                    _ => 3,
                };
                need += usize::from(rank == 0);
                let a = ago(now, s.map_or(0, |s| s.last_ts));
                let lane = s.and_then(|s| s.lane.as_deref()).unwrap_or_default();
                let head = gh::cached_pr(ctx, f, n)["headRefOid"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let chips = s.map_or(String::new(), |s| stage_line(fc, s, &head));
                // #205: a lane herdr still lists opens its Lanes sheet (serve.js `.ql`); a gone one is muted text.
                let link = |a: &str, text: String| match ls.iter().find(|l| l.name == a) {
                    Some(_) => format!("<a href=#lanes class=\"ql nw el\" data-agent=\"{}\">{text}</a>", esc(a)),
                    None => format!("<span class=\"ql nw el m\">{text}</span>"),
                };
                let mut seen = BTreeSet::new();
                let spawns = events.iter().rev().filter(|e| e.is("spawn", f, n));
                let qls: Vec<String> = spawns
                    .filter_map(|e| e.str("agent").filter(|a| seen.insert(*a)).map(|a| (a, e.ts)))
                    .map(|(a, ts)| {
                        let st = ls.iter().find(|l| l.name == a).map_or("gone", |l| l.state.as_str());
                        let (dot, tn) = (
                            if st == "working" { "●" } else { "○" },
                            if st == "gone" { "m" } else { tone(st) },
                        );
                        link(
                            a,
                            format!("<span class={tn}>{dot}</span> {} {st} {}", esc(a), ago(now, ts)),
                        )
                    })
                    .collect();
                let more = (qls.len() > 5).then(|| {
                    format!(
                        "<details class=qm><summary>+{} older</summary>{}</details>",
                        qls.len() - 5,
                        qls[5..].concat()
                    )
                });
                let qls = (!qls.is_empty()).then(|| {
                    format!(
                        "<div class=qls>{}{}</div>",
                        qls[..qls.len().min(5)].concat(),
                        more.unwrap_or_default()
                    )
                });
                // The current lane name links only while herdr lists it; otherwise it stays plain text.
                let lane = if ls.iter().any(|l| l.name == lane) {
                    link(lane, esc(lane))
                } else {
                    esc(lane)
                };
                let mut card = vec![chips, lane, pr_line(ctx, Some(fc), f, n, s)];
                if is_dead {
                    card.push("<span class=r>dead: the log has this lane open, herdr has no such agent</span>".into());
                }
                if let Some(b) = blocked_by {
                    card.push(format!("<span class=r>blocked by #{b}</span>"));
                }
                // #99: a decision taken by default, with [change]: a new decision on the same ask.
                let last = events.iter().rev().find(|e| e.is("decision", f, n));
                if let Some(d) = last.filter(|d| d.str("by") == Some("default")) {
                    let asked = |e: &&Event| e.is("needs-decision", f, n) && e.str("id") == d.str("id");
                    let ask = events.iter().find(asked).filter(|_| rw);
                    let change = ask.map_or(String::new(), |a| answer_form(ctx, a, "change"));
                    let choice = esc(d.str("choice").unwrap_or("?"));
                    card.push(format!("<span>decided by default: {choice}</span>{change}"));
                }
                if let (true, false, Some(k)) = (rw, is_dead, k) {
                    let mv = |label: &str, key: &str, to: u64| {
                        format!("<form method=post>{}<input type=hidden name=issue value={n}><input type=hidden name={key} value={to}><button>{label}</button></form>", hidden("mv", f, &mt))
                    };
                    let up = if k > 0 {
                        mv("▲ up", "before", q[k - 1].issue)
                    } else {
                        String::new()
                    };
                    let down = q.get(k + 1).map(|x| mv("▼ down", "after", x.issue)).unwrap_or_default();
                    card.push(format!("<span class=btns>{up}{down}</span>"));
                }
                let s = k.filter(|&k| q[k].serial).map(|_| format!(" data-s=\"{sl}\""));
                let b = blocked_by.map_or(String::new(), |b| format!(" data-b={b}"));
                let data = format!(
                    "class=\"q\"{}{b}{}",
                    if k.is_some() { " data-q" } else { "" },
                    s.unwrap_or_default()
                );
                let row = row("q", "", &format!("#{n}"), state, &a, &card).replacen("class=\"q\"", &data, 1);
                let row = match (qls, row.strip_suffix("</details>")) {
                    (Some(q), Some(r)) => format!("{r}{q}</details>"),
                    _ => row,
                };
                let gh = format!(
                    "<span><a href=\"https://github.com/{}/issues/{n}\">#{n}</a></span>",
                    esc(&fc.repo)
                );
                let row = row.replacen(&format!("<span>#{n}</span>"), &gh, 1);
                rows.push((
                    rank,
                    match titles(ctx, f, n) {
                        Some(t) => {
                            row.replacen("</summary>", &format!("<span class=tl>{}</span></summary>", esc(&t)), 1)
                        }
                        None => row,
                    },
                ));
            }
            rows.sort_by_key(|r| r.0);
            let mut body: String = rows.into_iter().map(|r| r.1).collect();
            if items.is_empty() {
                body += "<p class=m>no open work</p>";
            }
            let mut done: Vec<&&IssueState> = mine
                .iter()
                .filter(|s| s.done() && now.saturating_sub(s.last_ts) < 86_400)
                .collect();
            done.sort_by_key(|s| std::cmp::Reverse(s.last_ts));
            if !done.is_empty() {
                let rows: String = done
                    .iter()
                    .take(5)
                    .map(|s| {
                        let pr = events
                            .iter()
                            .rev()
                            .find(|e| e.is("merged", f, s.issue))
                            .and_then(|e| e.str("pr"));
                        let link = pr.map_or(String::new(), |p| {
                            format!(
                                " <a href=\"https://github.com/{}/pull/{}\">PR #{}</a>",
                                esc(&fc.repo),
                                esc(p),
                                esc(p)
                            )
                        });
                        format!(
                            "<p>#{} <span class=g>{}</span> {}{link}</p>",
                            s.issue,
                            s.state,
                            age(now.saturating_sub(s.last_ts))
                        )
                    })
                    .collect();
                body += &format!(
                    "<details class=done><summary><i class=cv></i><span>done today: {}</span></summary>{rows}</details>",
                    done.len()
                );
            }
            let button = |op: &str, label: &str| {
                if rw {
                    format!(
                        "<form method=post>{}<button>{label}</button></form>",
                        hidden(op, f, &mt)
                    )
                } else {
                    String::new()
                }
            };
            let (k, fe) = (fc.color(f), esc(f));
            let need = if need > 0 {
                format!("<span class=\"tag r\">! {need} need you</span>")
            } else {
                String::new()
            };
            // An export has no `/frag`: it carries each fleet's [config] sheet inline.
            if self.export.is_some() {
                body += &format!(
                    "<template data-cfg=\"{fe}\">{}</template>",
                    config_sheet(ctx, cfg, f, fc, (rw, &mt), st)
                );
            }
            let lanes: Vec<&&IssueState> = mine.iter().filter(|s| s.active_lane().is_some()).collect();
            let plural = |n: usize| if n == 1 { "" } else { "s" };
            if fc.paused {
                let nd = lanes
                    .iter()
                    .filter(|s| gone(s).is_some_and(|l| l.state == "dead"))
                    .count();
                let what = match (lanes.len(), nd) {
                    (0, _) => "no lane".to_string(),
                    (n, d) if d == n => format!("{n} lane{} (dead)", plural(n)),
                    (n, 0) => format!("{n} lane{}", plural(n)),
                    (n, d) => format!("{n} lanes ({d} dead)"),
                };
                let since = events
                    .iter()
                    .rev()
                    .find(|e| e.kind == "pause" && e.fleet == *f && e.issue.is_none());
                let since = since.map_or(String::new(), |e| format!(" · {}", age(now.saturating_sub(e.ts))));
                cards.push((!need.is_empty(), format!(
                    "<details class=\"fc off k-{k}\"{sa}><summary><i class=cv></i><span class=fn><b>{fe}</b> <i class=m>●</i> paused{since} · {what}</span>{need}{}</summary>{body}</details>",
                    button("resume", "Resume")
                )));
                continue;
            }
            let drive = button("drive", "Drive now");
            let stall = cfg.limits.stall_minutes * 60;
            let live = lanes.iter().filter(|s| gone(s).is_none());
            let next = live.map(|s| ((s.last_ts + stall).saturating_sub(now), s.issue)).min();
            let mut meta = vec![match next {
                Some((0, n)) => format!("stall check: #{n} is due now"),
                Some((left, n)) => format!("stall check: #{n} in {}", age(left)),
                None => "no live lane".into(),
            }];
            if !lanes.is_empty() {
                meta.push(format!("{} lane{}", lanes.len(), plural(lanes.len())));
            }
            if !fc.serial_paths.is_empty() {
                let busy = queue::serial_in_flight(events, st, f).len();
                meta.push(format!("serial {busy}/{}", fc.serial_max.unwrap_or(1)));
            }
            // #173: a refused pipeline stops this fleet only; its card says so in red.
            let bad = match fc.bad.len() {
                0 => String::new(),
                n => format!(
                    "<p class=r>! BAD_PIPELINE · {n} error{} in fleet.toml · nothing moves until it is fixed</p>",
                    if n == 1 { "" } else { "s" }
                ),
            };
            cards.push((!need.is_empty(), format!(
                "<div class=\"fc k-{k}\"{sa}><div class=fh><span class=fn><b>{fe}</b>{need}</span>\
                 <span class=btns>{drive}{}<button type=button class=cfg data-f=\"{fe}\">Config</button></span></div>{}{bad}{body}</div>",
                button("pause", "Pause"),
                drive_line(ctx, events, f, &meta),
            )));
        }
        cards.sort_by_key(|c| !c.0);
        cards.into_iter().map(|c| c.1).collect()
    }

    /// herdr's agents joined with the log (None when herdr cannot be read).
    fn lanes(&self, now: u64, cfg: &Config, events: &[Event], st: &States) -> Option<Vec<lanes::Lane>> {
        let list = self.herdr_json(now, "agent")?;
        let agents = list["result"]["agents"].as_array().cloned().unwrap_or_default();
        let wsl = self.herdr_json(now, "workspace").unwrap_or_default();
        let ws: BTreeMap<String, String> = wsl["result"]["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| Some((w["workspace_id"].as_str()?.into(), w["label"].as_str()?.into())))
            .collect();
        Some(lanes::join(
            cfg,
            &self.cached(now, 30),
            &self.ctx.worktrees,
            events,
            st,
            &agents,
            &ws,
        ))
    }

    fn waits(&self, ctx: &Ctx, cfg: &Config, events: &[Event], all: &[lanes::Lane]) -> Vec<crate::waiting::Waiting> {
        let live: Vec<_> = all
            .iter()
            .map(|l| json!({"name": l.name, "agent_status": l.state}))
            .collect();
        crate::waiting::scan(ctx, cfg, &self.cached(ctx.now, 30), events, &live, false)
            .into_iter()
            .map(|x| x.1)
            .collect()
    }

    /// #37: Herd mode's data (`/herd.json`, the `herd` event; docs/serve.md).
    /// `weather` (loopback only) forces the host values.
    fn herd(&self, ctx: &Ctx, weather: Option<&str>) -> Result<serde_json::Value> {
        let events = read(&ctx.state)?;
        let (cfg, st) = (Config::load(&ctx.config)?, fold(&events));
        let all = self.lanes(ctx.now, &cfg, &events, &st).unwrap_or_default();
        let waits = self.waits(ctx, &cfg, &events, &all);
        let lanes = |f: Option<&String>| -> Vec<_> {
            all.iter().filter(|l| l.fleet.as_ref() == f).map(|l| {
                let (title, tone, shown, a) = view(ctx, l, &waits);
                json!({"name": l.name, "issue": l.issue, "title": title, "role": l.role, "harness": l.harness,
                       "model": l.model, "effort": l.effort, "account": l.account, "state": l.state, "shown": shown, "tone": tone, "age": a, "coat": coat(&l.name)})
            }).collect()
        };
        let queued = |f: &String| -> Vec<_> {
            let open = |s: &IssueState| s.state == "queued" || s.state == "needs-human";
            let waiting = queue::view(&events, &st, f)
                .into_iter()
                .filter(|i| st.get(&(f.clone(), i.issue)).is_none_or(open));
            waiting.map(|i| json!({"issue": i.issue, "title": gh::cached_title(ctx, f, i.issue), "blocked_by": i.blocked_by, "coat": coat(&format!("{f}#{}", i.issue))})).collect()
        };
        let fleets: Vec<_> = cfg
            .fleet
            .keys()
            .map(|f| json!({"name": f, "lanes": lanes(Some(f)), "queue": queued(f), "needs": st.values().filter(|s| &s.fleet == f && s.state == "needs-human").map(|s| s.issue).collect::<Vec<_>>()}))
            .collect();
        let lim = &cfg.limits;
        let sky = [
            ("calm", 0.1, "normal"),
            ("gusty", 0.7, "normal"),
            ("storm", 1.3, "normal"),
            ("fog", 0.3, "warn"),
        ];
        let forced = sky.iter().find(|w| Some(w.0) == weather).map(|w| (w.1, w.2));
        let h = forced.map_or_else(
            || self.sample(),
            |(k, p)| Host {
                load_per_cpu: Some(k * lim.load_per_cpu),
                pressure: Some(p.into()),
                swapout: Some(0.0),
                ..Host::default()
            },
        );
        let host = json!({"load": h.load_per_cpu, "limit": lim.load_per_cpu, "pressure": h.pressure, "swapouts": h.swapout, "swap_limit": lim.swapout_rate, "gate": host::admit(&h, lim).err()});
        Ok(json!({"fleets": fleets, "others": lanes(None), "host": host}))
    }

    /// State sections, fleet subgroups inside; a row opens the bottom sheet (serve.js) with its hidden card.
    fn lanes_tab(&self, ctx: &Ctx, cfg: Option<&Config>, events: &[Event], st: &States, paired: bool) -> String {
        let Some(cfg) = cfg else {
            return "<p class=m>no fleet.toml</p>".into();
        };
        let Some(all) = self.lanes(ctx.now, cfg, events, st) else {
            return "<p class=r>herdr agent list failed</p>".into();
        };
        let state = |l: &lanes::Lane| l.issue.zip(l.fleet.as_ref()).and_then(|(n, f)| st.get(&(f.clone(), n)));
        let waits = self.waits(ctx, cfg, events, &all);
        let mut h = String::new();
        for (sec, subs) in lanes::group(&all, |l| state(l).map_or(l.since, |s| s.last_ts.max(l.since))) {
            h += &format!(
                "<h2><span class={}>{sec} ({})</span></h2>",
                tone(sec),
                subs.iter().map(|s| s.1.len()).sum::<usize>()
            );
            for (repo, ls) in subs {
                h += &format!("<h3>{} ({})</h3>", esc(&repo), ls.len());
                for l in ls {
                    h += &self.lane_row(ctx, cfg, l, state(l), paired, &waits);
                }
            }
        }
        if h.is_empty() {
            "<p class=m>no agents</p>".into()
        } else {
            h
        }
    }

    fn lane_row(
        &self,
        ctx: &Ctx,
        cfg: &Config,
        l: &lanes::Lane,
        s: Option<&IssueState>,
        paired: bool,
        waits: &[crate::waiting::Waiting],
    ) -> String {
        let screen = l
            .pane
            .as_ref()
            .and_then(|p| herdr::read(&self.cached(ctx.now, 30), p).ok());
        let (title, tone, shown, a) = view(ctx, l, waits);
        let issue = l.issue.map(|n| format!("#{n}")).unwrap_or_default();
        let round = s.map(|s| {
            format!(
                "round {}",
                s.state
                    .split_once('(')
                    .map_or(s.rounds.to_string(), |r| r.1.replace(')', ""))
            )
        });
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        let facts = [
            l.role.clone(),
            o(&round),
            l.harness.clone(),
            o(&l.model),
            o(&l.effort),
            o(&l.account),
            o(&l.port),
            a.clone(),
        ];
        let mut card = vec![
            esc(format!("{issue} {}", o(&title)).trim()),
            esc(&join(&facts.iter().map(String::as_str).collect::<Vec<_>>())),
        ];
        if let (Some(n), Some(f)) = (l.issue, &l.fleet) {
            card.push(pr_line(ctx, cfg.fleet.get(f), f, n, s));
        }
        if l.state == "dead" {
            card.push("<span class=r>⚠ log/pane mismatch: the log has this lane open, herdr has no pane</span>".into());
        }
        card.insert(1, esc(&shown));
        let card = self::card(&card)
            + &if l.state == "blocked" {
                self.blocked(ctx, l, paired)
            } else {
                String::new()
            };
        let act = esc(&lanes::activity(&l.harness, &screen.unwrap_or_default()));
        // #53: line 1 name (one line) + `state age`; line 2 the reason (no age), or `superseded by` on an issue's older lane.
        let w = wait_of(l, waits);
        let sup = s.and_then(|s| s.lane.as_ref()).filter(|x| **x != l.name && w.is_some());
        let why = match (sup, &w) {
            (Some(x), _) => format!("<span class=\"why m\">superseded by {}</span>", esc(x)),
            (None, Some(w)) => format!("<span class=\"why {tone}\">{}</span>", esc(&w.what)),
            _ => String::new(),
        };
        let tone = if sup.is_some() { "m" } else { tone };
        let since = w.filter(|_| sup.is_none()).map_or(l.since, |w| w.since);
        let a = ago(ctx.now, since);
        format!(
            "<div class=lane data-agent=\"{}\" data-role={}><button type=button class=lr>{}<span class=\"nw el\">{}</span><span class=\"nw {}\">{} {a}</span>\
             {why}<span class=act>{act}</span></button><div class=card hidden>{card}</div></div>",
            esc(&l.name),
            esc(&l.role) + if l.pane.is_none() { " data-gone" } else { "" },
            avatar(&l.name, &l.state),
            esc(&l.name),
            tone,
            esc(&l.state)
        )
    }

    fn sample(&self) -> Host {
        lock(&self.host).clone()
    }

    /// #141: four groups, MACHINE, SERVICES, FLEETS and ALERTS, of rows with a status (`data-t`); serve.js ranks them.
    fn host_tab(&self, ctx: &Ctx, cfg: Option<&Config>, events: &[Event]) -> String {
        let (hs, r, dl) = (self.sample(), &self.cached(ctx.now, 60), Limits::default());
        let lim = cfg.map_or(&dl, |c| &c.limits);
        let up = age(ctx.now.saturating_sub(*STARTED.get_or_init(|| ctx.now)));
        let board = format!("up {up} · v{}", env!("CARGO_PKG_VERSION"));
        let rows = cfg.map_or(vec![], |c| crate::timer::services(ctx, c, r, Some(board)));
        let (mut m, mut s, mut f, mut a) = (String::new(), String::new(), String::new(), String::new());
        // #168: swap-out's gauge is the window's median (the rule, with pressure warn/critical); `now` is one sample.
        let avg = hs.swap_median.map_or("?".into(), |m| format!("{m:.0}"));
        for (k, x, g, l, p) in [
            ("load/cpu", hs.load_per_cpu, hs.load_per_cpu, lim.load_per_cpu, 2),
            (
                "swap-out",
                hs.swapout,
                hs.swap_median.or(hs.swapout),
                lim.swapout_rate,
                0,
            ),
        ] {
            let (t, bar, pct) = gauge(g.unwrap_or(0.0) / l, "▏");
            let x = x.map_or("?".into(), |x| format!("{x:.p$}"));
            let unit = match p {
                0 => format!(
                    " now · {avg} avg <span class=mt>pages/2s · held on the avg with pressure warn/critical</span>"
                ),
                _ => String::new(),
            };
            let o = if t == "r" { "▶ " } else { "" };
            m += &kv(
                k,
                t,
                &format!("{bar} <b class={t}>{o}{x}</b> <span class=m>/ {l}</span>{pct}{unit}"),
            );
        }
        let p = hs.pressure.as_deref().unwrap_or("?");
        m += &kv("memory", tone(p), &format!("<b class={}>{}</b>", tone(p), esc(p)));
        if let Some(gb) = host::free_gb(r) {
            let (t, bar, _) = gauge(lim.min_free_gb / gb, "┃");
            let (min, o) = (lim.min_free_gb, if t == "r" { "▶ " } else { "" });
            let fl = format!("<span class=\"mt fx\">· floor {min} GB</span>");
            m += &kv("disk", t, &format!("{bar} <b class={t}>{o}{gb:.0} GB free</b> {fl}"));
        }
        // #164: the worktree total from the last walk (and its age, #160) against the budget, and folders of no
        // fleet under the worktree root, removed only from here (paired) or `clean --orphans --yes`.
        if let Some(c) = cfg {
            let budget = lim.worktree_budget_gb;
            let (gb, at) = crate::check::worktree_du(ctx).unzip();
            let (t, bar, pct) = gauge(gb.unwrap_or(0.0) / budget, "▏");
            let o = if t == "r" { "▶ " } else { "" };
            let x = gb.map_or("?".into(), |gb| format!("{gb:.1}"));
            let at = at.flatten().map_or(String::new(), |at| {
                format!(" <span class=\"mt fx\">· {} ago</span>", ago(ctx.now, at))
            });
            let val = format!("{bar} <b class={t}>{o}{x}</b> <span class=m>/ {budget} GB</span>{pct}{at}");
            m += &kv("fleet disk", t, &val);
            let n = crate::check::orphan_dirs(ctx, c).len();
            if n > 0 && self.addr.is_some() {
                let (hid, s) = (hidden("orphans", "", &mtime(ctx)), if n == 1 { "" } else { "s" });
                m += &format!("<form method=post>{hid}<button data-confirm=\"Remove {n} orphan folder{s} under the worktree root?\">remove orphans</button> <span class=m>{n} folder{s} of no fleet</span></form>");
            }
        }
        // #186: processes still running in closed lanes' worktrees; `[reap]` TERMs, then KILLs them.
        let left = cfg.zip(herdr::agents(r).ok()).map_or(vec![], |(c, a)| {
            crate::check::leftovers(ctx, c, r, &a, false).unwrap_or_default()
        });
        if !left.is_empty() {
            let pids = left.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(",");
            let kb: f64 = crate::runner::ok(r, "ps", &["-o", "rss=", "-p", &pids]).map_or(0.0, |s| {
                s.split_whitespace().filter_map(|k| k.parse::<f64>().ok()).sum()
            });
            let n = left.len();
            let btn = format!("<form method=post>{}<button data-confirm=\"TERM, then KILL, {n} leftover process(es)?\">reap</button></form>", hidden("reap", "", &mtime(ctx)));
            let btn = if self.addr.is_some() { btn } else { String::new() };
            let val = format!(
                "<b class=a>{n} running</b> <span class=mt>{:.0} MB · in closed lanes' worktrees</span>{btn}",
                kb / 1024.0
            );
            m += &kv("leftovers", "a", &val);
        }
        m += &match (cfg, host::admit(&hs, lim)) {
            (None, _) => String::new(),
            (_, Ok(())) => kv("spawns", "g", "<b class=g>admitted</b>"),
            (_, Err(why)) => {
                let (why, x) = (esc(&why), "<span class=x><b class=r>held</b> · no new lanes while");
                let raw = format!("<pre>spawn gate HOST_BUSY: {why}</pre>");
                hrow("spawns", "r", &format!("{x} {why}</span>"), "", &raw)
            }
        };
        // #98: green, amber or red per service, what clears it otherwise.
        let svc = |s: &&crate::timer::Row| ["board", "tick", herdr::BIN].contains(&s.0.as_str());
        for (k, t, d, fix) in rows.iter().filter(svc) {
            let fix = [String::new(), format!(" <span class=\"mt fx\">→ {}</span>", chips(fix))];
            s += &kv(
                k,
                t,
                &format!("<b class={t}>{}</b>{}", esc(d), fix[usize::from(*t != "g")]),
            );
        }
        // #172: the Herd's theme; amber when its theme.json is invalid (farm shows).
        let (_, name, ok) = theme(ctx, "theme.json");
        let bad = format!(
            "<b class=a>farm</b> <span class=mt>`{}` has no valid theme.json</span>",
            esc(&name)
        );
        s += &if ok {
            kv("theme", "g", &format!("<b>{}</b>", esc(&name)))
        } else {
            kv("theme", "a", &bad)
        };
        if let Some(c) = cfg.filter(|c| !c.accounts.names.is_empty()) {
            let capped = |a: &&String| crate::access::capped_until(c, events, a, ctx.now).is_some();
            let all = c.accounts.names.len();
            let n = c.accounts.names.keys().filter(capped).count();
            let t = ["g", "a", "r"][usize::from(n > 0) + usize::from(n == all)];
            let mut sub = String::new();
            for name in c.accounts.names.keys() {
                let cap = crate::access::cap_line(c, self.r, events, name, ctx.now);
                let usage = crate::usage::meter(ctx, c, self.r, name)
                    .map(|m| crate::usage::shown(self.r, &m, &c.accounts.usage[name], ctx.now));
                let usage = match usage {
                    Some(Ok((parts, pace))) => {
                        let bar = |(t, p): &(String, f64)| {
                            format!(
                                " · {} <meter min=0 max=100 low=75 high=89.9 optimum=0 value={p:.0}></meter>",
                                esc(t)
                            )
                        };
                        parts.iter().map(bar).collect::<String>()
                            + &pace.map_or(String::new(), |p| format!(" · <b class=a>{}</b>", esc(&p)))
                    }
                    Some(Err(hint)) => format!(" · <span class=a>{}</span>", esc(&hint)),
                    None => String::new(),
                };
                let hours = crate::usage::lane_hours(events, name, ctx.now);
                sub += &format!(
                    "<p>{} · <b class={}>{}</b>{usage} <span class=mt>{}</span></p>",
                    esc(name),
                    tone(cap.split(' ').next().unwrap_or_default()),
                    esc(&cap),
                    esc(&hours)
                );
            }
            let x = format!("<span class=x><b class={t}>{n} of {all} capped</b></span>");
            s += &hrow("accounts", t, &x, "", &format!("<div class=sub>{sub}</div>"));
        }
        s += &match cfg.and_then(|c| crate::usage::deepseek(ctx, c, self.r)) {
            Some(Ok((b, low))) => {
                let (t, word) = if low { ("a", "low") } else { ("g", "ok") };
                let d = format!("{} {:.2}", esc(&b.currency), b.total);
                kv("deepseek", t, &format!("{d} · <b class={t}>{word}</b>"))
            }
            Some(Err(hint)) => kv("deepseek", "a", &esc(&hint)),
            None => String::new(),
        };
        // Each fleet's last `driven` result in plain words, the raw line on tap; an owner's pause is grey (no problem).
        for (name, fc) in cfg.map(|c| &c.fleet).into_iter().flatten() {
            let last = events.iter().rfind(|e| e.kind == "driven" && e.fleet == *name);
            let (t, w, x) = match last {
                _ if fc.paused => ("m", "paused", format!("`herdsman resume {name}` starts it again")),
                Some(e) => in_words(e, fc.serial_label()),
                None => ("m", "nothing yet", String::new()),
            };
            let sep = [" · ", " "][usize::from(t == "g" || x.is_empty())];
            let val = format!("<span class=x><b class={t}>{w}</b>{sep}{}</span>", chips(&x));
            let e = last.filter(|_| !fc.paused);
            let ago = e.map_or(String::new(), |e| age(ctx.now.saturating_sub(e.ts)));
            let raw = e
                .and_then(|e| e.str("result"))
                .map(|r| format!("<pre>{}</pre>", esc(r)));
            f += &hrow(name, t, &val, &ago, &raw.unwrap_or_default());
        }
        // #87: on/off from fleet.toml; the last `push-sent`, unless a webpush `notify-failed` is newer.
        let (t, d) = crate::timer::phone(ctx, cfg, events);
        a += &kv("push", t, &format!("<b class={t}>{}</b>", esc(&d)));
        if self.addr.is_some() && self.export.is_none() {
            // #18: the button shows only in the Home Screen app (serve.js); endpoints and keys stay on the Mac.
            let key = crate::webpush::vapid(&ctx.state)
                .map(|k| crate::webpush::public(&k))
                .unwrap_or_default();
            let key = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, key);
            a += &format!("<button id=push type=button hidden data-key={key}>Enable alerts</button>");
            for s in crate::webpush::load(&ctx.state) {
                a += &format!(
                    "<form method=post action=/push/unsubscribe class=role><input type=hidden name=id value={}><span>{} <span class=mt>alerts on {} ago</span></span><button>Remove</button></form>",
                    s.id(),
                    esc(&s.label),
                    age(ctx.now.saturating_sub(s.created))
                );
            }
            let (all, mt) = (
                devices::load(ctx)
                    .into_iter()
                    .filter(|d| d.code.is_none())
                    .collect::<Vec<_>>(),
                mtime(ctx),
            );
            if all.is_empty() {
                a += "<p class=mt>no paired device yet: tap Pair this phone, then `herdsman serve approve &lt;code&gt;` here</p>";
            }
            for d in all {
                let when = format!(
                    "paired {} ago · used {} ago",
                    age(ctx.now.saturating_sub(d.created)),
                    age(ctx.now.saturating_sub(d.used))
                );
                a += &format!("<form method=post class=role>{}<input type=hidden name=hash value={}><span>{} <span class=m>{when}</span></span><button>Remove</button></form>", hidden("unpair", "", &mt), esc(&d.hash), esc(&d.label));
            }
        }
        // #183: the label column is the longest label's width, at most 11ch (serve.css `--kw`).
        let ks = [&m, &s, &f, &a].map(|g| {
            g.split("<span class=k>")
                .skip(1)
                .map(|k| k.split('<').next().unwrap_or_default().chars().count())
                .max()
                .unwrap_or(0)
        });
        let head = "<details class=grp style=--kw:@ch><summary><i class=dt></i><span class=gn>{}</span><span class=gs></span><i class=cv></i></summary>";
        let head = head.replace('@', &ks.iter().max().map_or(0, |k| *k.min(&11)).to_string());
        let grp = |(n, rows): (&str, String)| format!("{}{rows}</details>", head.replace("{}", n));
        let gs = [("MACHINE", m), ("SERVICES", s), ("FLEETS", f), ("ALERTS", a)];
        gs.map(grp).concat()
    }
}

/// #141: a 12-cell text gauge of `x` (value ÷ limit; disk: floor ÷ free): (status, the bar, how far past the limit).
/// Green under 80 %, amber to 100 %, red past it (the value then leads with `▶`); `end` closes it (disk's floor tick).
fn gauge(x: f64, end: &str) -> (&'static str, String, String) {
    let t = ["g", "a", "r"][usize::from(x >= 0.8) + usize::from(x > 1.0)];
    let n = (1..=12).filter(|i| f64::from(*i) <= x * 12.0 + 0.5).count();
    let (full, empty) = ("█".repeat(n), "░".repeat(12 - n));
    let pct = Some(x).filter(|x| *x > 1.0).map(|x| format!(" ({:.0}%)", x * 100.0));
    let bar = format!("<span class=\"bar {t}\">▕{full}<i>{empty}</i>{end}</span>");
    (t, bar, pct.unwrap_or_default())
}

/// #141: a Host row with status `t`: its label, then `cells`; with `more` (a raw line, the accounts) it opens on tap.
fn hrow(k: &str, t: &str, cells: &str, age: &str, more: &str) -> String {
    let k = format!("<span class=k>{}</span>", esc(k));
    if more.is_empty() {
        return format!("<p class=gr data-t={t}>{k}{cells}</p>");
    }
    let t = format!("data-t={t}><summary>{k}{cells}<span class=t>{age}<i class=cv></i></span>");
    format!("<details class=rw {t}</summary>{more}</details>")
}

/// A Host row of a label and one value.
fn kv(k: &str, t: &str, val: &str) -> String {
    hrow(k, t, &format!("<span class=w>{val}</span>"), "", "")
}

/// Escaped text with each `command` as a code chip.
fn chips(s: &str) -> String {
    let part = |(i, p): (usize, &str)| [esc(p), format!("<code>{}</code>", esc(p))][i % 2].clone();
    s.split('`').enumerate().map(part).collect()
}

/// #141: a `driven` result as the Host tab says it, by code (`ok:<step>` when it ran): status, word, the plain rest with
/// what clears it. `{why}`/`{then}`: the raw reason's first two `;` parts; `{busy}`: the issues in the way.
const WORDS: &str = "HOST_BUSY|a|held|the host is busy: {why} · starts again when it drops
DISK_LOW|a|held|{why} GB floor · `herdsman clean --yes`
ALL_CAPPED|a|waiting|{why} · {then}
CAP|a|waiting|{why} · starts when a lane finishes
SERIAL_BUSY|a|waiting|{slots} full ({busy} must merge first)
ROUND_LIMIT UPDATE_LIMIT|r|needs you|#{n}: {why} · your call on its card
WORKTREE_DIRTY|r|needs you|#{n}: {why} · `herdsman kill` it, or spawn `--supersede`
CI_NOT_GREEN|a|waiting|#{n}'s CI is not green yet
NO_VERDICT STALE_VERDICT VERDICT_FAIL|a|waiting|#{n} has no PASS for its head · a new round
PAUSED|m|paused|{then}
EXIT4|g|started|#{n} · prompt not confirmed yet
ok:merge|g|merged|#{n}
ok:clean|g|cleaned|merged lanes
ok:ci-red|a|sent back|#{n}: red CI, a fix round next
ok:review|g|started|review #{n}
ok:fix|g|started|fix #{n}
ok:conflict|g|started|update #{n}
ok:|g|started|#{n}";

/// #141: a `driven` event in plain words (WORDS, else `failed` and the raw `result`); `serial`: the fleet's label.
pub fn in_words(e: &Event, serial: &str) -> (&'static str, &'static str, String) {
    let (raw, n) = (e.str("result").unwrap_or_default(), e.issue.unwrap_or(0).to_string());
    let body = raw.split_once(": ").map_or(raw, |x| x.1);
    let (why, then) = body.split_once("; ").unwrap_or((body, ""));
    let busy: Vec<_> = raw.split(' ').filter(|w| w.starts_with('#')).collect();
    let noun = format!("serial ({serial})").replace("serial (serial)", "serial");
    let slots = format!("{} {noun} slot{}", busy.len(), ["s", ""][usize::from(busy.len() == 1)]);
    let code = match e.str("code") {
        Some("EXIT") if raw.starts_with("exit 4:") => "EXIT4".into(),
        Some(c) => c.to_string(),
        None => format!("ok:{}", e.str("step").unwrap_or_default()),
    };
    let ok = |l: &&str| code.starts_with("ok:") && l.starts_with("ok:|");
    let row = |l: &&str| l.split('|').next().unwrap_or_default().split(' ').any(|c| c == code) || ok(l);
    let w = WORDS.lines().find(row).unwrap_or("|r|failed|{raw}");
    let w: Vec<_> = w.split('|').collect();
    let x = w[3].replace("{why}", why).replace("{then}", then).replace("{n}", &n);
    let x = x.replace("{busy}", &busy.join(", ")).replace("{slots}", &slots);
    (w[1], w[2], x.replace("{raw}", raw))
}

/// `--demo`'s runner (#39): `runner.json`'s stdout for the longest matching command-line prefix, else exit 1.
#[derive(Default)]
struct Canned(BTreeMap<String, String>);

impl Runner for Canned {
    fn run(&self, prog: &str, args: &[String]) -> Result<crate::runner::Out> {
        let line = format!("{prog} {}", args.join(" "));
        let out = self
            .0
            .iter()
            .filter(|(k, _)| line.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len());
        Ok(crate::runner::Out {
            code: i32::from(out.is_none()),
            stdout: out.map_or(String::new(), |o| o.1.clone()),
            stderr: String::new(),
        })
    }
    fn sleep(&self, _: u64) {}
}

/// #172: the files a theme folder may hold; `/theme/<file>` serves nothing else.
const THEME: [&str; 3] = ["sprites.svg", "theme.json", "scene.svg"];
/// The active theme's `file` (`<config dir>/themes/<name>/`, then the built-in `<name>`, then farm's), the theme's
/// name and whether its theme.json parses; it does not (or no such theme) → farm.
fn theme(ctx: &Ctx, file: &str) -> (Option<Vec<u8>>, String, bool) {
    let name = Config::load(&ctx.config)
        .ok()
        .and_then(|c| c.serve.theme)
        .unwrap_or_else(|| "farm".into());
    let get = |n: &str, f: &str| {
        let own = ctx.config.with_file_name("themes").join(n).join(f);
        let built = ASSETS
            .iter()
            .find(|a| a.0 == format!("themes/{n}/{f}"))
            .map(|a| a.2.to_vec());
        (!n.contains(['/', '\\', '.']))
            .then(|| std::fs::read(own).ok())
            .flatten()
            .or(built)
    };
    let ok = get(&name, "theme.json").is_some_and(|j| serde_json::from_slice::<serde_json::Map<_, _>>(&j).is_ok());
    let n = if ok { name.as_str() } else { "farm" };
    (get(n, file).or_else(|| get("farm", file)), name, ok)
}

/// `serve --export <dir> [--demo]` (#39, docs/serve.md): the page and its files as static files with relative URLs.
pub fn export(ctx: &Ctx, r: &dyn Runner, dir: &std::path::Path, demo: bool) -> Result<()> {
    let canned = if demo {
        Canned(serde_json::from_str(&std::fs::read_to_string(
            ctx.config.with_file_name("runner.json"),
        )?)?)
    } else {
        Canned::default()
    };
    let (r, events): (&dyn Runner, _) = (if demo { &canned } else { r }, read(&ctx.state)?);
    let ctx = &Ctx {
        now: if demo {
            events.iter().map(|e| e.ts).max().unwrap_or(0) + 60
        } else {
            ctx.now
        },
        ..ctx.clone()
    };
    let s = Server {
        export: Some(demo),
        host: Arc::new(Mutex::new(host::sample(r))),
        ..Server::new(ctx, r, demo.then(|| "demo".into()))
    };
    let (page, herd) = (
        s.page(ctx, &BTreeMap::new(), demo)?
            .replace("<form method=post", "<form inert"),
        s.herd(ctx, None)?.to_string(),
    );
    let (sw, v) = (SW.replace("@V", &format!("{VER:08x}")), format!("a/{VER:08x}/"));
    // #63: the demo-only `?audit=<view>` UI gate (scripts/ui-audit.sh); never in the live page.
    let page = if demo {
        page.replace(
            "</body>",
            &format!("<script>{}</script></body>", include_str!("../assets/ui-audit.js")),
        )
    } else {
        page
    };
    std::fs::create_dir_all(dir.join("lane"))?;
    std::fs::create_dir_all(dir.join("theme"))?;
    let files = [
        ("index.html", page.as_bytes()),
        ("herd.json", herd.as_bytes()),
        ("sw.js", sw.as_bytes()),
    ]
    .map(|(n, b)| (n.to_string(), b));
    for (name, body) in files
        .into_iter()
        .chain(FILES.iter().map(|f| (f.0.to_string(), f.2)))
        .chain(ASSETS.iter().map(|a| (v.clone() + a.0, a.2)))
    {
        std::fs::create_dir_all(dir.join(&name).with_file_name(""))?;
        std::fs::write(dir.join(name), body)?;
    }
    for f in THEME {
        theme(ctx, f)
            .0
            .map_or(Ok(()), |b| std::fs::write(dir.join("theme").join(f), b))?;
    }
    let (cfg, st) = (Config::load(&ctx.config)?, fold(&events));
    for l in s
        .lanes(ctx.now, &cfg, &events, &st)
        .unwrap_or_default()
        .into_iter()
        .filter(|_| demo)
    {
        let safe = !l.name.starts_with('.') && l.name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        if let Some(j) = s
            .transcript(ctx, &BTreeMap::from([("agent".to_string(), l.name.clone())]))
            .filter(|_| safe)
        {
            std::fs::write(dir.join("lane").join(format!("{}.json", l.name)), j)?;
        }
    }
    Ok(())
}

/// #169: `[issue] [PR #M] [checks]` wherever an issue shows, from the caches only: `checks` is the PR's checks
/// page, else the issue branch's latest run (`gh::run`, read by the tick), else none.
fn links(ctx: &Ctx, repo: &str, fleet: &str, issue: u64, pr: Option<u64>) -> String {
    let (gh, a) = (format!("https://github.com/{}", esc(repo)), |u: &str, t: &str| {
        format!("[<a href=\"{u}\">{t}</a>]")
    });
    let mut l = vec![a(&format!("{gh}/issues/{issue}"), "issue")];
    if let Some(m) = pr {
        l.push(a(&format!("{gh}/pull/{m}"), &format!("PR #{m}")));
        l.push(a(&format!("{gh}/pull/{m}/checks"), "checks"));
    } else if let Some(u) = gh::cached_run(ctx, fleet, issue) {
        l.push(a(&esc(&u), "checks"));
    }
    l.join(" ")
}

/// `[issue] [PR #11] [checks] · CI ✓ · rev PASS r1`, from the cached `herdsman pr` read only (never calls `gh`).
fn pr_line(ctx: &Ctx, fc: Option<&crate::config::Fleet>, fleet: &str, issue: u64, s: Option<&IssueState>) -> String {
    let pr = gh::cached_pr(ctx, fleet, issue);
    let n = pr["number"].as_u64();
    let mut parts: Vec<String> = fc.map(|c| links(ctx, &c.repo, fleet, issue, n)).into_iter().collect();
    if pr.is_null() {
        parts.push("no PR read yet".to_string());
    } else {
        if fc.is_none() {
            parts.push(format!("PR #{}", n.unwrap_or_default()));
        }
        let ci = gh::ci(&gh::required(
            &pr["statusCheckRollup"],
            fc.map(|c| c.required_checks.as_slice()).unwrap_or_default(),
        ));
        let mark = match ci {
            "green" => "✓",
            "failed" => "✗",
            _ => "…",
        };
        parts.push(format!(
            "CI <span class={}>{mark}</span>",
            tone(if ci == "failed" {
                "FAIL"
            } else if ci == "green" {
                "PASS"
            } else {
                ""
            })
        ));
    }
    if let Some((s, (_, v, _))) = s.and_then(|s| s.verdicts.last().map(|v| (s, v))) {
        parts.push(format!("rev <span class={}>{}</span> r{}", tone(v), esc(v), s.rounds));
    }
    parts.join(" · ")
}

/// The queue row's dim line from the caches only: the issue title, then the PR's when it differs; one 390 px line.
fn titles(ctx: &Ctx, fleet: &str, issue: u64) -> Option<String> {
    let spec = gh::cached_title(ctx, fleet, issue);
    let pr = gh::cached_pr(ctx, fleet, issue);
    let pr = pr["number"]
        .as_u64()
        .zip(pr["title"].as_str())
        .filter(|p| Some(p.1) != spec.as_deref())
        .map(|(n, p)| format!("PR #{n} · {p}"));
    let t = [spec, pr].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    if t.is_empty() {
        return None;
    }
    if t.chars().count() <= 50 {
        return Some(t);
    }
    let cut: String = t.chars().take(51).collect();
    Some(format!(
        "{}…",
        cut.rsplit_once(' ')
            .map_or(cut.as_str(), |c| c.0)
            .trim_end_matches([' ', '·'])
    ))
}

/// #162: a fleet's [config] sheet: GENERAL, LIMITS, ROLES (the `set` pickers when `rw`), ALERTS, HOSTS / NEEDS.
/// Only the role pickers write (through `set`); the rest is read-only, edited in the config file named on top.
fn config_sheet(
    ctx: &Ctx,
    cfg: &Config,
    fleet: &str,
    fc: &crate::config::Fleet,
    (rw, mt): (bool, &str),
    st: &States,
) -> String {
    let tag = |role: &str| {
        let on: Vec<String> = fc
            .flow()
            .stage
            .iter()
            .filter(|s| s.role.as_deref() == Some(role))
            .map(|s| format!("{}: {}", s.kind, s.name))
            .collect();
        let on = if role == "design" {
            "owner only".into()
        } else {
            on.join(", ")
        };
        if on.is_empty() {
            String::new()
        } else {
            format!("<span class=\"tag m\">{}</span> ", esc(&on))
        }
    };
    let path = ctx.config.display().to_string();
    let home = crate::config::home().display().to_string();
    let path = path.strip_prefix(&home).map_or(path.clone(), |p| format!("~{p}"));
    let file = ctx
        .config
        .file_name()
        .map_or("fleet.toml".into(), |f| f.to_string_lossy().into_owned());
    let edit = format!("edit {}", esc(&file));
    let (l, all) = (&cfg.limits, "<span class=\"tag m\">all fleets</span>");
    let none = "<span class=m>none</span>";
    let list = |v: &[String]| {
        if v.is_empty() {
            none.to_string()
        } else {
            esc(&v.join(", "))
        }
    };
    let cap = |n: Option<usize>| n.map_or("<span class=m>no cap</span>".into(), |n| n.to_string());
    let line_cap = std::fs::read_to_string(fc.local_path().join("ci/cap.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("fleet")?.get(fleet)?.get("line_cap")?.as_integer());
    let notify = |n: &[crate::config::Notify]| {
        let n: Vec<String> = n
            .iter()
            .map(|n| {
                let on = if n.on.is_empty() {
                    "default events".into()
                } else {
                    n.on.join(", ")
                };
                format!("{} ({on})", if n.webpush() { "web push" } else { "command" })
            })
            .collect();
        if n.is_empty() {
            none.to_string()
        } else {
            esc(&n.join("; "))
        }
    };
    let group = |name: &str, hint: &str, rows: &[(&str, String)]| {
        let rows: String = rows.iter().map(|(k, v)| format!("<dt>{k}</dt><dd>{v}</dd>")).collect();
        format!("<section class=cg><h3>{name}<span>{hint}</span></h3><dl class=kv2>{rows}</dl></section>")
    };
    let k = fc.color(fleet);
    let general = [
        ("repo", esc(&fc.repo)),
        ("local", esc(&fc.local)),
        ("integration", esc(&fc.integration)),
        ("driver", "herdsman <span class=m>(herdsman drive)</span>".into()),
        (
            "default effort",
            format!(
                "{}{all}",
                l.effort
                    .as_deref()
                    .map_or("<span class=m>unset → none</span>".into(), esc)
            ),
        ),
        (
            "colour",
            format!(
                "<span class={k}>■</span> --{k} <span class=m>({})</span>",
                if fc.color.is_some() { "set" } else { "auto" }
            ),
        ),
        ("env", list(&fc.env.keys().cloned().collect::<Vec<_>>())),
        (
            "node_modules",
            if fc.clone_node_modules {
                "cloned".into()
            } else {
                "<span class=m>not cloned</span>".into()
            },
        ),
    ];
    let rounds = |n: u32, on: &str| {
        if fc.flow().builtin {
            format!("{n} <span class=m>→ {on}, see Pipeline</span>{all}")
        } else {
            format!("<span class=m>n/a (pipeline)</span>{all}")
        }
    };
    let limits = [
        // #173: the built-in pipeline's defaults; a fleet with its own pipeline sets them per stage.
        ("review rounds", rounds(l.review_rounds, "rev's rounds")),
        ("update rounds", rounds(l.update_rounds, "gates' rounds")),
        ("stall after", format!("{} min{all}", l.stall_minutes)),
        ("ask default", format!("{} min{all}", l.ask_default_minutes)),
        (
            "max lanes",
            format!("impl {} · rev {}", cap(fc.max_impl), cap(fc.max_rev)),
        ),
        (
            "line cap",
            line_cap.map_or("<span class=m>none in ci/cap.toml</span>".into(), |n| {
                format!("{n} <span class=m>(ci/cap.toml in the repo)</span>")
            }),
        ),
        ("serial paths", list(&fc.serial_paths)),
        (
            "deny commands",
            list(&fc.deny(l).iter().map(|d| d.cmd().to_string()).collect::<Vec<_>>()),
        ),
        (
            "serial",
            format!(
                "{} at a time · label 🔒{}",
                fc.serial_max.unwrap_or(1),
                esc(fc.serial_label())
            ),
        ),
        (
            "required checks",
            if fc.required_checks.is_empty() {
                "<span class=m>all</span>".into()
            } else {
                list(&fc.required_checks)
            },
        ),
        ("merge label", fc.merge_label.as_deref().map_or(none.into(), esc)),
    ];
    let own = if fc.notify.is_empty() {
        "<span class=m>global only</span>".into()
    } else {
        notify(&fc.notify)
    };
    let alerts = [("notify", own), ("global", notify(&cfg.notify))];
    let roles: String = fc
        .role
        .iter()
        .map(|(name, r)| role_form(cfg, fleet, name, r, rw, mt).replacen("</b> ", &format!("</b> {}", tag(name)), 1))
        .collect();
    format!(
        "<p class=cp><span class=m>config</span> {}</p>{}{}{}<section class=cg><h3>Roles<span>set: harness · model · effort</span></h3>{roles}\
         <p class=rk>profile, accounts, mcp: {edit}</p></section>{}<section class=cg><h3>Hosts / needs<span>after #157</span></h3>\
         <p class=soon>Which hosts this fleet's lanes may run on, matched by host tags against the issue's needs. Shown here once #157 lands.</p></section>",
        esc(&path),
        group("General", &edit, &general),
        pipeline_group(fleet, fc, st),
        group("Limits", &edit, &limits),
        group("Alerts", &edit, &alerts),
    )
}

/// A role's pickers (`rw`): harness, model (disabled while `models` is unset), effort, through the `set` op; else text.
fn role_form(cfg: &Config, fleet: &str, role: &str, r: &Role, rw: bool, mt: &str) -> String {
    let head = format!(
        "<p class=rh><b>{}</b><span class=m>{}</span></p>",
        esc(role),
        esc(&r.profile_or(role))
    );
    let names = |v: &[String]| {
        if v.is_empty() {
            "none".into()
        } else {
            esc(&v.join(", "))
        }
    };
    let rest = format!(
        "<p class=rk>accounts {} · mcp {}</p>",
        names(&r.account_list()),
        names(&r.mcp)
    );
    if !rw {
        let v = join(&[
            &r.harness,
            r.model.as_deref().unwrap_or_default(),
            r.effort.as_deref().unwrap_or_default(),
        ]);
        return format!("<div class=role>{head}<p>{}</p>{rest}</div>", esc(&v));
    }
    // Unset: what the lane actually gets (the profile's/harness's model, `[limits].effort`).
    let model = if r.harness == "hermes" {
        "profile".to_string()
    } else {
        r.harness.clone()
    };
    let effort = cfg.limits.effort.clone().unwrap_or_else(|| "none".into());
    let select = |name: &str, cur: &Option<String>, def: &str, opts: &[&str], attr: &str| {
        let mut s = format!("<select name={name} aria-label={name}{attr}>");
        if cur.as_ref().is_none_or(|c| !opts.contains(&c.as_str())) {
            s += &format!(
                "<option value=\"\" selected>{}</option>",
                esc(&cur.clone().unwrap_or(format!("default ({def})")))
            );
        }
        for o in opts {
            let sel = if cur.as_deref() == Some(o) { " selected" } else { "" };
            s += &format!("<option{sel}>{}</option>", esc(o));
        }
        s + "</select>"
    };
    let models: Vec<&str> = cfg.models.iter().map(String::as_str).collect();
    // No `models`: the picker shows the current model, disabled (`set` refuses any other), with the fix.
    let (off, hint) = if models.is_empty() {
        (" disabled data-off", "<p class=rk>add models = [...] to fleet.toml</p>")
    } else {
        ("", "")
    };
    // The effort list follows the harness (serve.js swaps it from data-e).
    let harness: String = crate::harness::NAMES
        .iter()
        .map(|h| {
            let sel = if *h == r.harness { " selected" } else { "" };
            format!(
                "<option{sel} data-e=\"{}\">{h}</option>",
                crate::harness::efforts(h).join(" ")
            )
        })
        .collect();
    format!(
        "<form method=post class=role>{}<input type=hidden name=role value={}>{head}<span class=rp>\
         <select name=harness aria-label=harness>{harness}</select>{}{}<button>Set</button></span>{hint}{rest}</form>",
        hidden("set", fleet, mt),
        esc(role),
        select("model", &r.model, &model, &models, off),
        select("effort", &r.effort, &effort, crate::harness::efforts(&r.harness), ""),
    )
}

/// #170: noise a run of folds into one `kind ×N` row (by kind, so a failure never shares a row with `clean`).
const NOISE: &[(&str, &str)] = &[
    ("clean", "lanes cleaned"),
    ("push-sent", "alerts sent"),
    ("notify-failed", "alerts that never arrived"),
];

type Words = (&'static str, &'static str, String, bool);

/// #170: an event as the Events tab says it: (glyph, tone, plain words, shown under `[important]`). `blocked` and
/// `driven` take #141's WORDS; the capacity holds stay out of `[important]` (herdsman waiting for room).
pub fn words(e: &Event, serial: &str) -> Words {
    let s = |k: &str| e.str(k).unwrap_or_default();
    let (r, round) = (
        e.str("reason").map_or(String::new(), |r| format!(" · {r}")),
        &e.data.get("round"),
    );
    let round = round.map_or(String::new(), |n| format!(" · round {n}"));
    match e.kind.as_str() {
        "merged" => ("✓", "g", format!("merged into {} · PR #{}", s("into"), s("pr")), true),
        "verdict" if s("verdict") == "PASS" => ("✓", "g", format!("review PASS{round}"), true),
        "verdict" => ("✗", "r", format!("review FAIL{round}"), true),
        "needs-human" => ("!", "r", format!("needs you{r}"), true),
        "needs-decision" => ("!", "r", format!("asks you ({}) · {}", s("id"), s("question")), true),
        "ask-waiting" => ("!", "r", format!("still waiting on you{r}"), true),
        "notify-failed" => (
            "✗",
            "r",
            format!("alert failed · the {} alert: {}", s("event"), s("error")),
            true,
        ),
        "lane-dead" => ("✗", "r", format!("lane gone{r}"), true),
        "hook-denied" => ("!", "a", format!("hook denied · {}", s("rule")), false),
        "blocked" | "driven" => {
            let mut b = e.clone();
            b.data.entry("result").or_insert_with(|| e.data["reason"].clone());
            let (t, w, x) = in_words(&b, serial);
            let txt = format!("{w} · {x}");
            let hold = matches!(
                e.str("code"),
                Some("HOST_BUSY" | "SERIAL_BUSY" | "CAP" | "ALL_CAPPED" | "PAUSED")
            );
            match (e.str("code"), t) {
                (None, _) => ("·", "m", format!("drive · {txt}"), false),
                (_, "r") if w == "needs you" => ("!", "r", txt, true),
                (_, "r") => ("✗", "r", txt, true),
                (_, "g") => ("▸", "l", txt, true),
                _ => ("⊘", "a", txt, !hold),
            }
        }
        "spawn" => ("▸", "l", format!("{} started{round}", s("role")), false),
        "push-sent" => ("↗", "m", format!("alert sent to {} phone(s)", e.data["devices"]), false),
        "clean" => ("·", "m", format!("lane cleaned ({})", s("why")), false),
        "decision" => ("✓", "m", format!("you answered {} · {}", s("id"), s("choice")), false),
        "override" => ("⊘", "m", format!("override{r}"), false),
        "queue" => {
            let n: Vec<String> = e.data["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|n| format!("#{n}"))
                .collect();
            ("·", "m", format!("queue · {} {}", s("op"), n.join(" ")), false)
        }
        k if [
            "stall",
            "orphan-lane",
            "conflict-predicted",
            "heads-up",
            "ready-unmerged",
        ]
        .contains(&k)
            || k.starts_with("orch-") =>
        {
            ("!", "a", format!("{k}{r}"), false)
        }
        k => ("·", "m", format!("{k}{r}"), false),
    }
}

/// #170: chips `[all] [important] [● fleet]…` (serve.js keeps the choice per phone), then each view's last 50 rows
/// (a fold is one row; `all`: every event, unfolded), newest first, under sticky time groups.
fn events_tab(ctx: &Ctx, r: &dyn Runner, cfg: Option<&Config>, events: &[Event], all: bool) -> String {
    let fc = |f: &str| cfg.and_then(|c| c.fleet.get(f));
    let tz = crate::usage::date(r, ctx.now, "+%z")
        .parse::<i64>()
        .map_or(0, |z| z / 100 * 3600 + z % 100 * 60);
    let local = |t: u64| t.cast_signed() + tz;
    let day = |t: u64| local(t).div_euclid(86_400);
    let group = |t: u64| match ctx.now.saturating_sub(t) {
        a if a < 300 => "now".to_string(),
        a if a < 3600 => "5m".into(),
        a if a < 7200 => "1h".into(),
        _ if day(t) == day(ctx.now) => "earlier today".into(),
        _ if day(t) + 1 == day(ctx.now) => "yesterday".into(),
        _ => {
            let (m, d) = crate::access::civil_month_day(day(t).max(0).cast_unsigned());
            let mon = "jan feb mar apr may jun jul aug sep oct nov dec"
                .split(' ')
                .zip(1..)
                .find(|x| x.1 == m);
            format!("{} {d}", mon.map_or("", |x| x.0))
        }
    };
    let mut order: Vec<&Event> = events.iter().rev().collect();
    order.sort_by_key(|e| std::cmp::Reverse(e.ts));
    let mut h = String::from("<div class=chips role=group aria-label=\"filter events\">");
    for (k, l) in [("all", "all"), ("imp", "important")] {
        h += &format!(
            "<button type=button data-k={k} aria-pressed={}><span>{l}</span></button>",
            k == "imp"
        );
    }
    for (name, f) in cfg.map(|c| &c.fleet).into_iter().flatten() {
        let (n, k) = (esc(name), f.color(name));
        h += &format!("<button type=button class=\"fc2 k-{k}\" data-fl=\"{n}\" aria-pressed=false><span><i class=fd></i>{n}</span></button>");
    }
    h += "</div>";
    for (v, imp) in [("imp", true), ("all", false)] {
        let mut rows: Vec<(Vec<&Event>, Words)> = vec![];
        for e in &order {
            let w = words(e, fc(&e.fleet).map_or("serial", |f| f.serial_label()));
            if imp && !w.3 {
                continue;
            }
            let noise = NOISE.iter().any(|n| n.0 == e.kind);
            let same = |r: &&mut (Vec<&Event>, Words)| {
                let p = r.0[0];
                p.fleet == e.fleet
                    && if noise {
                        p.kind == e.kind
                    } else {
                        p.ts == e.ts && r.1 .2 == w.2
                    }
            };
            if let Some(r) = rows.last_mut().filter(|r| !all && same(r)) {
                r.0.push(e);
                continue;
            }
            if rows.len() == 50 && !all {
                break;
            }
            rows.push((vec![e], w));
        }
        let back = rows.last().map(|r| r.0[0].ts).map_or(String::new(), |t| {
            let l = local(t).rem_euclid(86_400);
            format!(" · back to {} {:02}:{:02}", group(t), l / 3600, l % 3600 / 60)
        });
        let more = if all {
            ""
        } else {
            " · <a href=\"?all=1\">all events, unfolded</a>"
        };
        let what = if imp { "important rows" } else { "rows" };
        h += &format!(
            "<div data-v={v}{}><p class=\"m es\">{} {what}{back}{more}</p>",
            [" hidden", ""][usize::from(imp)],
            rows.len()
        );
        let mut i = 0;
        while i < rows.len() {
            let g = group(rows[i].0[0].ts);
            let n = rows[i..].iter().take_while(|r| group(r.0[0].ts) == g).count();
            h += &format!("<div class=eg><h3><span>{g}</span><span>{n}</span></h3>");
            for (evs, (gl, t, w, imp)) in &rows[i..i + n] {
                let e = evs[0];
                let k = fc(&e.fleet).map_or(String::new(), |f| format!(" k-{}", f.color(&e.fleet)));
                let num = match (fc(&e.fleet), e.issue) {
                    (Some(f), Some(n)) => format!(
                        "<a class=n href=\"https://github.com/{}/issues/{n}\">#{n}</a>",
                        esc(&f.repo)
                    ),
                    (_, n) => format!("<span class=n>{}</span>", n.map_or(String::new(), |n| format!("#{n}"))),
                };
                let solo = evs.len() == 1 || e.issue.is_some() && evs.iter().all(|x| x.issue == e.issue);
                let w = match NOISE.iter().find(|n| n.0 == e.kind) {
                    Some((k, l)) if evs.len() > 1 => format!("{k} ×{} · {l}", evs.len()),
                    _ if solo => w.clone(),
                    _ => w.replacen(" · ", &format!(" ×{} · ", evs.len()), 1),
                };
                let pr = e
                    .data
                    .get("pr")
                    .and_then(|p| p.as_u64().or_else(|| p.as_str()?.parse().ok()));
                let gl2 = fc(&e.fleet).zip(e.issue).map_or(String::new(), |(f, n)| {
                    let pr = pr.or_else(|| gh::cached_pr(ctx, &e.fleet, n)["number"].as_u64());
                    format!("<span>{}</span>", links(ctx, &f.repo, &e.fleet, n, pr))
                });
                let l = local(e.ts).rem_euclid(86_400);
                let at = format!(
                    "{:02}:{:02}:{:02} · {} · {}",
                    l / 3600,
                    l % 3600 / 60,
                    l % 60,
                    esc(&e.fleet),
                    esc(&e.kind)
                );
                let via = if e.str("via") == Some("web") { " (web)" } else { "" };
                let body = if evs.len() > 1 {
                    let li: String = evs.iter().map(|x| format!("<li>{}</li>", esc(&plain(x)))).collect();
                    format!("<ul class=fl2>{li}</ul>")
                } else {
                    format!("<pre>{}</pre>", esc(&raw(e)))
                };
                h += &format!(
                    "<details class=\"er{k}{}\" data-f=\"{}\"><summary><i class=\"kg t-{t}\">{gl}</i><i class=fd></i>{num}<span class=sm>{}{via}</span><span class=ag>{}</span></summary><div class=ex><p class=lk>{gl2}<span class=m>{at}</span></p>{body}</div></details>",
                    [" imp", ""][usize::from(!imp)],
                    esc(&e.fleet),
                    esc(&w),
                    age(ctx.now.saturating_sub(e.ts))
                );
            }
            h += "</div>";
            i += n;
        }
        h += "</div>";
    }
    h
}

/// A check role's lane (`rev`, or #173 any check stage's): read-only from the page.
fn reviewer(ctx: &Ctx, fleet: &str, role: &str) -> bool {
    let cfg = Config::load(&ctx.config).ok();
    role == "rev" || cfg.and_then(|c| Some(c.fleet.get(fleet)?.family(role) == "rev")) == Some(true)
}

/// #173: where an issue is in its fleet's pipeline: (stage, the mode or round its chip shows); `None` off the graph.
fn stage_at<'a>(fc: &'a crate::config::Fleet, s: &'a IssueState) -> Option<(&'a str, String)> {
    let p = fc.flow();
    let build = p.stage_of(s.build.as_deref().unwrap_or(p.start()));
    let n = s.state.split(['(', ')']).nth(1).unwrap_or_default();
    match s.state.split('(').next()? {
        "impl" => Some((build, String::new())),
        "fix" if s.updating => Some((build, "update".into())),
        "fix" => Some((build, "fix".into())),
        "review" => Some((s.stage.as_str(), format!("r{n}"))),
        "ready" | "blocked" => Some((p.gate(), String::new())),
        _ => None,
    }
}

/// Kind → (glyph class, glyph): ▲ build, ◆ check, ◫ gate, ✓ merge, ! park.
fn glyph(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "build" => ("b", "▲"),
        "check" => ("c", "◆"),
        "gate" => ("g", "◫"),
        "merge" => ("m", "✓"),
        _ => ("x", "!"),
    }
}

/// #173: the card's "where it is now": `○ ─ ▲ impl ─ ◆ rev ─ ◫ gates ─ ✓ merge`, the current stage lit, the stages
/// before it plain, a check with a PASS at the PR head ticked.
fn stage_line(fc: &crate::config::Fleet, s: &IssueState, head: &str) -> String {
    let Some((at, mode)) = stage_at(fc, s) else {
        return String::new();
    };
    let p = fc.flow();
    let pos = p.stage.iter().position(|x| x.name == at).unwrap_or(0);
    let chips = p.stage.iter().enumerate().map(|(i, x)| {
        let (k, g) = glyph(&x.kind);
        let ok = x.kind == "check"
            && s.verdicts
                .iter()
                .rev()
                .find(|v| v.2 == x.name && v.0 == head)
                .is_some_and(|v| v.1 == "PASS");
        let tick = if ok && i != pos { "<b class=g>✓</b>" } else { "" };
        let cls = match i.cmp(&pos) {
            std::cmp::Ordering::Less => "pt dn".to_string(),
            std::cmp::Ordering::Equal => format!("pt on k{k}"),
            std::cmp::Ordering::Greater => "pt".into(),
        };
        let name = if i == pos && !mode.is_empty() {
            format!("{} · {mode}", x.name)
        } else {
            x.name.clone()
        };
        format!(
            "<span class=\"{cls}\"><i class=\"kd {k}\">{g}</i>{}{tick}</span>",
            esc(&name)
        )
    });
    let all: Vec<String> = std::iter::once("<span class=\"pt dn\"><i class=kd>○</i></span>".to_string())
        .chain(chips)
        .chain(["<span class=pt><i class=\"kd m\">✓</i>merge</span>".to_string()])
        .collect();
    format!("<span class=ps>{}</span>", all.join("<i class=sx>─</i>"))
}

/// #173: the fleet's Pipeline group as data (`data-pipe`); serve.js draws it to docs/design/pipeline: the validity line
/// or the broken rules, the graph with `n here` per stage, the legend, the pipeline as fleet.toml text.
fn pipeline_group(fleet: &str, fc: &crate::config::Fleet, st: &States) -> String {
    let p = fc.flow();
    let mine: Vec<&IssueState> = st.values().filter(|s| s.fleet == fleet && !s.done()).collect();
    let count = |f: &dyn Fn(&IssueState) -> bool| mine.iter().filter(|s| f(s)).count();
    let stages: Vec<serde_json::Value> = (p.stage.iter())
        .map(|s| {
            let run = s.role.as_deref().and_then(|r| fc.role.get(r)).map(|r| {
                let m = r.model.as_deref().unwrap_or("default");
                format!("{} · {m} · effort {}", r.harness, r.effort.as_deref().unwrap_or("none"))
            });
            let here = count(&|x| stage_at(fc, x).is_some_and(|a| a.0 == s.name));
            json!({"name": s.name, "kind": s.kind, "role": s.role, "run": run, "next": s.next, "rounds": s.rounds,
                "line": s.line, "here": here, "loop": p.loop_of(&s.name)})
        })
        .collect();
    let bad: Vec<serde_json::Value> = (fc.bad.iter())
        .map(|b| json!({"rule": b.rule, "msg": b.msg, "stage": b.stage, "line": b.line}))
        .collect();
    let checks: Vec<&str> = p.checks().map(|c| c.name.as_str()).collect();
    let d = json!({"fleet": fleet, "into": fc.integration, "builtin": p.builtin, "stages": stages, "bad": bad,
        "checks": checks, "required": fc.required_checks, "queued": count(&|s| s.state == "queued"),
        "parked": count(&|s| s.state == "needs-human")});
    format!(
        "<section class=\"cg pp\" data-pipe=\"{}\"><h3>Pipeline</h3></section>",
        esc(&d.to_string())
    )
}

/// #170: an event as pretty JSON, a multi-line string as its first line and `… (N more lines)`, 160 chars at most.
fn raw(e: &Event) -> String {
    let mut e = e.clone();
    for x in e.data.values_mut() {
        if let Some(s) = x.as_str().filter(|s| s.contains('\n') || s.chars().count() > 160) {
            let n = s.lines().count().saturating_sub(1);
            let one = s.lines().next().unwrap_or_default();
            let mut t: String = one.chars().take(160).collect();
            t += &[String::new(), "…".into()][usize::from(one.chars().count() > 160)];
            t += &[String::new(), format!(" … ({n} more lines)")][usize::from(n > 0)];
            *x = t.into();
        }
    }
    serde_json::to_string_pretty(&e).unwrap_or_default()
}
