use super::super::{form, lock, shown, Config, Ctx, Result, Server};
use super::{hosts::HOST_ACTIONS, page_places, permitted};
use crate::events::Json;
use crate::runner::Runner;
use serde_json::{json, Value};
use std::sync::Mutex;

const OFF: [&str; 3] = ["/screens/scene", "/screens/threads", "/screens/proxy-token"];
const CARDS: &str = "/screens/cards";

pub(in super::super) enum Who<'a> {
    Device(&'a str),
    Plugin,
    Local,
}

impl Server<'_> {
    pub(in super::super) fn perform(&self, ctx: &Ctx, who: &Who, a: &Value) -> Result<Value> {
        let name = a["action"].as_str().unwrap_or_default();
        let owner = !matches!(who, Who::Plugin);
        let secret = matches!(who, Who::Local) && name == "secret-card" && a.at("values").at("act") == "save";
        if secret {
            return crate::gate::refused("NOT_PERMITTED", "a secret is entered on a paired device only");
        }
        if !permitted(name) && (!owner || !HOST_ACTIONS.contains(&name)) {
            let why = format!("`{name}` is not in the manifest's actions");
            return crate::gate::refused("NOT_PERMITTED", why);
        }
        let by = match who {
            Who::Device(l) => l,
            Who::Plugin => "cooee",
            Who::Local => "owner",
        };
        match name {
            "later" => self.hide_card(ctx, by, &a["values"]),
            n if HOST_ACTIONS.contains(&n) => self.host_op(ctx, n, &a["values"]),
            _ => self.action(ctx, name, a),
        }
    }
}

/// A place the terminal offers: a root screen of the Board (`root`), else a menu row opened as a sheet.
pub struct Place {
    pub label: String,
    pub subtitle: String,
    pub path: String,
    pub root: bool,
}

/// An action's reply as the terminal uses it: its toast, whether its sheet closes, the screen it replaced.
pub struct Done {
    pub toast: String,
    pub close: bool,
    pub screen: Option<Value>,
}

/// The phone's screens and actions in this process, as the paired owner: no socket, no HTTP, no serve.
pub struct Local<'a>(Server<'a>, Mutex<u64>);

fn thread_id(t: &str) -> bool {
    t.strip_prefix('t')
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

fn off(v: &Value) -> bool {
    let to = |k: &str| v.at("tap").at(k).as_str().or(v.at(k).as_str());
    ["sheet", "open", "screen"]
        .iter()
        .any(|k| to(k).is_some_and(|p| OFF.contains(&p.split('?').next().unwrap_or_default())))
}

fn prune(v: &mut Value) {
    match v {
        Value::Array(a) => {
            a.retain(|x| !off(x));
            a.iter_mut().for_each(prune);
        }
        Value::Object(m) => {
            if let Some(t) = m
                .get("tap")
                .map(|t| t.at("thread"))
                .and_then(Value::as_str)
                .filter(|t| !thread_id(t))
            {
                let lane = json!({ "sheet": format!("/screens/lanes/{t}") });
                m.insert("tap".into(), lane);
            }
            m.values_mut().for_each(prune);
        }
        _ => {}
    }
}

impl<'a> Local<'a> {
    /// The door over `ctx`'s state, reading through `r`; nothing listens.
    pub fn new(ctx: &'a Ctx, r: &'a dyn Runner) -> Self {
        Local(Server::new(ctx, r, Some("local".into())).deferring(), Mutex::new(0))
    }

    fn at(&self, now: u64) -> Ctx {
        Ctx {
            now,
            ..self.0.ctx.clone()
        }
    }

    /// The places, in the phone's order, without those the terminal does not offer (Herd, Threads), with Workflow.
    pub fn places() -> Vec<Place> {
        let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
        let mut out: Vec<Place> = (page_places().as_array().into_iter().flatten())
            .filter(|p| p["screen"].is_string() && !off(p))
            .map(|p| Place {
                label: s(p, "label"),
                subtitle: s(p, "subtitle"),
                path: s(p, "screen"),
                root: p["type"] == "header-strip",
            })
            .collect();
        out.push(Place {
            label: "Workflow".into(),
            subtitle: "the issue statechart, live counts".into(),
            path: "/screens/workflow".into(),
            root: false,
        });
        out
    }

    /// The Board's first screen: the queue, of `fleet` alone when given.
    pub fn home(fleet: Option<&str>) -> String {
        fleet.map_or("/screens/queue".into(), |f| {
            format!("/screens/queue?repo={}", super::super::enc(f))
        })
    }

    /// The asks and cards above the root screen.
    pub fn cards() -> String {
        CARDS.into()
    }

    /// The tree `GET <path>` returns at `now`, pruned; `None` when the path builds nothing.
    pub fn screen(&self, now: u64, path: &str) -> Option<Value> {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        if OFF.contains(&p) {
            return None;
        }
        let ctx = self.at(now);
        let mut v = crate::runner::request(|| self.0.build_screen(&ctx, p, &form(q), true))?;
        if p == CARDS {
            v = json!({"type": "screen", "id": "cards", "title": "asks and cards", "blocks": v});
        }
        prune(&mut v);
        Some(v)
    }

    /// `POST /action` with `body`, run to its end; a refusal or an error as its line.
    pub fn act(&self, now: u64, body: &Value) -> Result<Done, String> {
        let res = self.0.perform(&self.at(now), &Who::Local, body);
        self.0.deliver();
        let v = res.map_err(|e| shown(&e))?;
        let ops = v.at("ops").as_array().into_iter().flatten();
        let screen = ops.filter(|o| o["op"] == "screen").find_map(|o| {
            let mut s = o["screen"].clone();
            prune(&mut s);
            s.is_object().then_some(s)
        });
        Ok(Done {
            toast: v.at("toast").as_str().unwrap_or_default().into(),
            close: v.at("close") == true,
            screen,
        })
    }

    /// Read again what serve's background threads read (shed, hosts), each when due.
    pub fn refresh(&self, now: u64) {
        let every = Config::kept(self.0.ctx).map_or(0, |c| c.limits.serve_host_secs);
        let due = {
            let mut at = lock(&self.1);
            let due = *at == 0 || now >= *at + every;
            *at = if due { now } else { *at };
            due
        };
        if due {
            *lock(&self.0.host) = crate::host::sample(self.0.r.r);
            self.0.r.sampled(now);
        }
        self.0.refresh(now);
    }

    /// The log's last seq and the snapshot's stamp: a screen may differ only when this moved.
    pub fn stamp(&self) -> (u64, Option<u64>) {
        let s = &self.0.ctx.state;
        (crate::db::last_seq(s), crate::db::stamp(s, "board"))
    }
}
