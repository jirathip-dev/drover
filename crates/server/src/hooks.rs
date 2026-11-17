use crate::config::Config;
use crate::handover::Handover;
use crate::runner::Runner;
use crate::{webhook, Ctx};
use anyhow::Result;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

/// `drover serve`: the board on `addr` plus, unless read-only, the Funnel listener on `[serve] hooks_addr`.
pub fn serve(ctx: &Ctx, r: &dyn Runner, addr: &str, read_only: bool) -> Result<()> {
    let res = board(ctx, r, addr, read_only, !read_only);
    if res.is_err() {
        crate::serve::started(ctx, "failed");
    }
    res
}

fn held(ctx: &Ctx, r: &dyn Runner, addr: &str, e: std::io::Error) -> Result<Option<TcpListener>> {
    let cfg = Config::kept(ctx).unwrap_or_default();
    let secs = cfg.limits.serve_handover_secs;
    let label = cfg.serve.launchd_label.as_deref().unwrap_or("dev.drover.serve");
    if crate::timer::job_pid(ctx, r, label).ok().flatten() == Some(drover_core::runner::pid()) {
        return take_over(r, addr, secs).map(Some);
    }
    let Some(sha) = crate::update::serving(addr, secs) else {
        return Err(e.into());
    };
    if sha == crate::SHA {
        drover_core::stderr!("drover serve: {addr} already serves {sha:.7}");
        return Ok(None);
    }
    let port = addr.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(0);
    let pid = crate::update::ask_handover(&crate::update::serve_sock(ctx, port), secs)
        .map_err(|h| anyhow::anyhow!("{e}; the serve there ({sha:.7}) did not hand over: {h:#}"))?;
    drover_core::stderr!("drover serve: {addr} ran {sha:.7}; handed over (bridge pid {pid})");
    Ok(None)
}

fn take_over(r: &dyn Runner, addr: &str, secs: u64) -> Result<TcpListener> {
    let pids = crate::timer::listeners(r, addr);
    for p in &pids {
        crate::runner::ok(r, "kill", &[&p.to_string()]).ok();
    }
    drover_core::stderr!("drover serve: {addr} was held by pid {pids:?}, not this job; stopped it");
    let t = drover_core::clock::instant();
    loop {
        match TcpListener::bind(addr) {
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && t.elapsed() < Duration::from_secs(secs.max(1)) => {
                drover_core::clock::sleep(Duration::from_millis(100));
            }
            res => return Ok(res?),
        }
    }
}

/// `serve`; `handover`: take over a predecessor's listeners and hand them to a successor (`crate::handover`).
pub fn board(ctx: &Ctx, r: &dyn Runner, addr: &str, read_only: bool, handover: bool) -> Result<()> {
    let mut got = if handover { crate::runner::inherited() } else { vec![] }.into_iter();
    let inherited = got.len() > 0;
    let l = match got.next().map_or_else(|| TcpListener::bind(addr), Ok) {
        Ok(l) => l,
        Err(e) if handover && e.kind() == std::io::ErrorKind::AddrInUse => match held(ctx, r, addr, e)? {
            Some(l) => l,
            None => return Ok(()),
        },
        Err(e) => return Err(e.into()),
    };
    let mode = if read_only { "read-only view" } else { "phone page" };
    drover_core::stderr!("drover serve: {mode} on http://{}/", l.local_addr()?);
    let hooks = Config::kept(ctx).map(|c| c.serve.hooks_addr()).unwrap_or_default();
    let hooks = (!read_only && !hooks.is_empty()).then(|| {
        got.next()
            .map_or_else(|| TcpListener::bind(&hooks), Ok)
            .map_err(|e| (hooks, e))
    });
    drop(got);
    let ls: Vec<_> = std::iter::once(&l)
        .chain(hooks.as_ref().and_then(|h| h.as_ref().ok()))
        .collect();
    let ho = handover.then(|| Handover::new(ctx, &ls, inherited)).transpose()?;
    let ho = ho.as_ref();
    std::thread::scope(|sc| {
        match &hooks {
            Some(Ok(h)) => {
                drover_core::stderr!("drover serve: webhooks on http://{}/hooks/github", h.local_addr()?);
                sc.spawn(move || listen(ctx, r, h, ho));
            }
            Some(Err((a, e))) => drover_core::stderr!("drover serve: no webhooks: {a}: {e}"),
            None => {}
        }
        if let Some(h) = ho {
            sc.spawn(move || h.watch_mem(ctx, r));
        }
        crate::serve::serve_with(ctx, r, l, read_only, ho)
    })
}

/// Serve `l` until a successor takes over (`ho`): a thread per delivery, at most 4 at once (then 503).
pub fn listen(ctx: &Ctx, r: &dyn Runner, l: &TcpListener, ho: Option<&Handover>) {
    let busy = &AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for mut s in l.incoming().flatten() {
            if busy.fetch_add(1, Relaxed) >= 4 {
                busy.fetch_sub(1, Relaxed);
                drop(s.write_all(&status(503, "busy")));
            } else {
                sc.spawn(move || {
                    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
                    let res = reply(ctx, r, &mut s);
                    drop(s.write_all(&res));
                    busy.fetch_sub(1, Relaxed);
                });
            }
            if ho.is_some_and(Handover::stopping) {
                break;
            }
        }
    });
    ho.map(Handover::left);
}

/// The response to the one request read from `s`. A body over GitHub's 25 MB is refused from its `Content-Length` before any of it is read.
pub fn reply(ctx: &Ctx, r: &dyn Runner, s: &mut dyn Read) -> Vec<u8> {
    let (mut buf, mut chunk) = (vec![], [0u8; 8192]);
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        match s.read(&mut chunk) {
            Ok(n) if n > 0 && buf.len() < 16_384 => buf.extend_from_slice(chunk.get(..n).unwrap_or_default()),
            _ => return status(400, "bad request"),
        }
    };
    let head = String::from_utf8_lossy(buf.get(..end).unwrap_or_default()).into_owned();
    let mut w = head.split_whitespace();
    let (method, path) = (w.next().unwrap_or_default(), w.next().unwrap_or_default());
    if path != "/hooks/github" {
        return status(404, "not found");
    }
    if method != "POST" {
        return status(405, "POST only");
    }
    let Some(n) = header(&head, "content-length").and_then(|n| n.parse::<usize>().ok()) else {
        return status(411, "length required");
    };
    if n > webhook::MAX_BODY {
        return status(413, "over GitHub's 25 MB");
    }
    let mut body = buf.split_off(end + 4);
    body.truncate(n);
    let left = u64::try_from(n - body.len()).unwrap_or(u64::MAX);
    if s.take(left).read_to_end(&mut body).is_err() || body.len() < n {
        return status(400, "short body");
    }
    let ctx = Ctx {
        now: crate::now(),
        ..ctx.clone()
    };
    let (event, sig) = (header(&head, "x-github-event"), header(&head, "x-hub-signature-256"));
    let (code, msg) = webhook::deliver(&ctx, r, event.unwrap_or_default(), sig, &body);
    status(code, &msg)
}

fn status(code: u16, msg: &str) -> Vec<u8> {
    let why = match code {
        200 => "OK",
        202 => "Accepted",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Bad Request",
    };
    let len = msg.len() + 1;
    format!("HTTP/1.1 {code} {why}\r\nContent-Type: text/plain\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{msg}\n")
        .into_bytes()
}

fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    head.lines().skip(1).find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
    })
}
