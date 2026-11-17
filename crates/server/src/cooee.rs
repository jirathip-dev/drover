use super::{shown, Config, Ctx, Refusal, Server, JSON};
use crate::orchestrator::chat;
use serde_json::{json, Value};

type Res = (&'static str, &'static str, Vec<u8>);

fn out(status: &'static str, v: &Value) -> Res {
    (status, JSON, v.to_string().into_bytes())
}

fn line(status: &'static str, l: &str) -> Res {
    out(status, &json!({ "line": l }))
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Started {
    thread: String,
    profile: String,
}

impl Server<'_> {
    pub(super) fn cooee(&self, ctx: &Ctx, head: &str, (method, p): (&str, &str), body: &str) -> Res {
        if !Self::plugin(ctx, head) {
            return line("401 Unauthorized", "the plugin token only");
        }
        let cfg = match Config::kept(ctx) {
            Ok(c) => c,
            Err(e) => return line("500 Internal Server Error", &shown(&e)),
        };
        let done = |r: anyhow::Result<String>| match r {
            Ok(l) => line("200 OK", &l),
            Err(e) if e.downcast_ref::<Refusal>().is_some() => line("409 Conflict", &shown(&e)),
            Err(e) => line("400 Bad Request", &shown(&e)),
        };
        match (method, p) {
            ("GET", "/cooee/profiles") => match chat::profiles(ctx, &cfg) {
                Ok(list) => out("200 OK", &json!({ "profiles": list })),
                Err(e) => line("500 Internal Server Error", &shown(&e)),
            },
            ("POST", _) if self.addr.is_none() => line("405 Method Not Allowed", "read-only: GET only"),
            ("POST", "/cooee/thread-started") => {
                let s: Started = serde_json::from_str(body).unwrap_or_default();
                done(chat::started(ctx, &cfg, &s.profile, &s.thread))
            }
            _ => line("404 Not Found", "no such cooee route"),
        }
    }
}
