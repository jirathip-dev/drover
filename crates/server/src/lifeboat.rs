use crate::api::Lane;
use crate::config::Config;
use crate::events::{age, open_asks, shared};
use crate::runner::Runner;
use crate::Ctx;
use std::collections::BTreeMap;
use std::fmt::Write;

const VIA: &str = "lifeboat";
const CSS: &str =
    ":root{color-scheme:light dark}body{font:16px/1.4 system-ui,sans-serif;margin:0 auto;padding:8px 12px;\
max-width:720px;overflow-wrap:anywhere}h1{font-size:20px}h2{font-size:18px;margin:16px 0 4px}section p{margin:8px 0}\
small{opacity:.7}form{display:inline}button{font:inherit;min-height:44px;min-width:44px;margin:2px 4px 2px 0}\
input{font:inherit;min-height:44px;width:100%;box-sizing:border-box;margin:2px 0}pre{white-space:pre-wrap;margin:4px 0}\
.b{font-weight:bold}";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn shell(body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport \
         content=\"width=device-width,initial-scale=1\"><title>drover lifeboat</title><style>{CSS}</style></head>\
         <body><h1>drover lifeboat</h1>{body}</body></html>"
    )
}

fn button(label: &str, fields: &[(&str, &str)]) -> String {
    let mut s = String::from("<form method=post action=lifeboat>");
    for (k, v) in fields {
        let _ = write!(s, "<input type=hidden name={k} value=\"{}\">", esc(v));
    }
    s + &format!("<button>{}</button></form>", esc(label))
}

/// An unpaired device's page: how to pair it (`devices.rs`, the same pairing as every drover page), nothing else.
pub fn hint() -> String {
    shell(
        "<p>Pair this device first: tap [pair], run the <code>drover serve approve</code> line with the code it \
         shows on the Mac, then come back, tap [claim] and open this page again.</p>\
         <form method=post action=/pair/request><button>pair</button></form>\
         <form method=post action=/pair/claim><button>claim</button></form>",
    )
}

/// After a post: back to the page, `msg` (url-encoded) its result line.
pub fn back(msg: &str) -> String {
    let url = format!("lifeboat?msg={msg}");
    shell(&format!(
        "<meta http-equiv=refresh content=\"0;url={url}\"><p><a href=\"{url}\">back</a></p>"
    ))
}

/// The page for a paired device: `lanes` are API v1's (the step's snapshot), `msg` the last post's result.
pub fn page(ctx: &Ctx, lanes: &[Lane], msg: &str) -> String {
    let (Ok(cfg), Ok(ev)) = (Config::kept(ctx), shared(&ctx.state)) else {
        return shell("<p>fleet.toml or the event log could not be read</p>");
    };
    let mut b = String::new();
    if !msg.is_empty() {
        let _ = write!(b, "<p class=b>{}</p>", esc(msg));
    }
    b += "<section><h2>Fleets</h2>";
    for (f, fc) in &cfg.fleet {
        let (word, op) = [("running", "pause"), ("paused", "resume")][usize::from(fc.paused)];
        let _ = write!(
            b,
            "<p>{} · {word} {}</p>",
            esc(f),
            button(op, &[("op", op), ("fleet", f)])
        );
    }
    b += "</section><section><h2>Queue</h2>";
    for f in cfg.fleet.keys() {
        for i in crate::queue::view(&ev, &ev.states, f) {
            let s = ev.states.get(&(f.clone(), i.issue));
            let title = crate::gh::cached_title(ctx, f, i.issue).unwrap_or_default();
            let state = s.map_or("queued", |s| &s.state);
            let lane = s.and_then(|s| s.active_lane()).map_or("no lane", |l| l.1);
            let ago = s.map_or(String::new(), |s| {
                format!(" · {}", age(ctx.now.saturating_sub(s.last_ts)))
            });
            let (f, title, lane) = (esc(f), esc(&title), esc(lane));
            let _ = write!(
                b,
                "<p>{f} #{} {title}<br><small>{state} · {lane}{ago}</small></p>",
                i.issue
            );
        }
    }
    b += "</section><section><h2>Lanes</h2>";
    for l in lanes {
        let host = if l.host.is_empty() { "this host" } else { &l.host };
        let why = if l.shown == l.state {
            String::new()
        } else {
            format!(" · {}", esc(&l.shown))
        };
        let (name, host, state) = (esc(&l.name), esc(host), esc(&l.state));
        let _ = write!(b, "<p>{name}<br><small>{host} · {state}{why}</small></p>");
    }
    b += "</section><section><h2>Asks</h2>";
    for a in open_asks(&ev) {
        let (fleet, id) = (a.fleet.as_str(), a.str("id").unwrap_or_default());
        let at = a.issue.map_or(String::new(), |n| format!(" #{n}"));
        let q = esc(a.str("question").unwrap_or_default());
        let _ = write!(b, "<p>{} {}{at}: {q}<br>", esc(fleet), esc(id));
        let opts = a.val("options").as_array().cloned().unwrap_or_default();
        for (n, o) in opts.iter().enumerate() {
            let (n, o) = ((n + 1).to_string(), o.as_str().unwrap_or_default());
            b += &button(
                &format!("{n}. {o}"),
                &[("op", "answer"), ("fleet", fleet), ("id", id), ("choice", &n)],
            );
        }
        let _ = write!(
            b,
            "</p><form method=post action=lifeboat><input type=hidden name=op value=answer>\
             <input type=hidden name=fleet value=\"{}\"><input type=hidden name=id value=\"{}\">\
             <input name=choice placeholder=\"answer in your words\" aria-label=answer><button>answer</button></form>",
            esc(fleet),
            esc(id)
        );
    }
    b += "</section><section><h2>Run cards</h2>";
    let open = |e: &&crate::events::Event| {
        let id = e.str("id").unwrap_or_default();
        e.kind == "run-card" && ctx.now <= e.ts + crate::card::TTL && crate::card::settled(&ev, id).is_none()
    };
    for c in ev.iter().filter(open) {
        let s = |k: &str| c.str(k).unwrap_or_default();
        let (id, sha) = (s("id"), &s("sha")[..12.min(s("sha").len())]);
        let (why, blast) = (esc(s("why")), esc(s("blast")));
        let _ = write!(
            b,
            "<p>{} {}<pre>{}</pre>why: {why}<br>touches: {blast}<br><small>sha {sha} · in {}</small><br>{}{}</p>",
            esc(&c.fleet),
            esc(id),
            esc(s("cmd")),
            esc(s("dir")),
            button("approve", &[("op", "approve"), ("id", id), ("sha", sha)]),
            button("deny", &[("op", "deny"), ("id", id)])
        );
    }
    shell(&(b + "</section>"))
}

/// A post's op, run through the CLI verb's own code (`via: lifeboat`); the line to show back.
pub fn act(ctx: &Ctx, r: &dyn Runner, f: &BTreeMap<String, String>) -> String {
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let mut out = Vec::new();
    let res = Config::kept(ctx).and_then(|cfg| match g("op") {
        op @ ("pause" | "resume") => crate::pause(ctx, &mut out, g("fleet"), op == "pause", Some(VIA)).map(|()| None),
        "answer" => {
            let who = (g("fleet"), g("id"));
            crate::answer(ctx, &cfg, r, &mut out, who, g("choice"), ("owner", Some(VIA))).map(|()| None)
        }
        op @ ("approve" | "deny") => {
            let yes = op == "approve";
            crate::card::settle_via(ctx, &cfg, r, g("id"), yes, yes.then(|| g("sha")), Some(VIA)).map(Some)
        }
        op => anyhow::bail!("unknown op `{op}`"),
    });
    let line = match res {
        Ok(l) => l.unwrap_or_else(|| String::from_utf8_lossy(&out).into_owned()),
        Err(e) => format!("{e:#}"),
    };
    line.lines().next().unwrap_or_default().chars().take(300).collect()
}
