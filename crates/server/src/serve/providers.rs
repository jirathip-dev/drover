//! #682: API v1's agent providers (#496–#498) for cooee-drover's fleets/system plugin: `providers`, each `[accounts]`
//! account and `DeepSeek` API key (a secret by its store name only, never its value), and the paired POSTs `sign-in`
//! (`claude setup-token` in a setup terminal, its token straight into the store), `key-add`, `key-check`, `key-remove`
//! (a key from the request body straight into the store, never into an event) and `use` (a config proposal card).
use super::{lanes, refused, shared, Config, Ctx, Event, Result, Server};
use crate::api::{Balance, Provider, Providers};
use crate::config::Role;
use crate::secret::{name_ok, service};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

impl Server<'_> {
    /// `GET providers`: the Host tab's account rows and `DeepSeek` balance, with the roles, fleets and lanes on each.
    pub(super) fn providers(&self, ctx: &Ctx) -> Result<Providers> {
        let (cfg, events) = (Config::load(&ctx.config)?, shared(&ctx.state)?);
        let all = self.lanes(ctx.now, &cfg, &events, &events.states).unwrap_or_default();
        let live: Vec<_> = all
            .iter()
            .filter(|l| !["dead", "parked"].contains(&l.state.as_str()))
            .collect();
        let count = |on: &dyn Fn(&&&lanes::Lane) -> bool| live.iter().filter(on).count() as u64;
        let on = |pick: &dyn Fn(&Role) -> bool| {
            let roles = cfg
                .fleet
                .iter()
                .flat_map(|(f, fc)| fc.role.iter().map(move |(n, r)| (f, n, r)));
            let roles: Vec<_> = roles.filter(|r| pick(r.2)).collect();
            let fleets: BTreeSet<_> = roles.iter().map(|r| r.0.clone()).collect();
            (
                roles.iter().map(|r| format!("{}.{}", r.0, r.1)).collect(),
                fleets.into_iter().collect(),
            )
        };
        let mut out = vec![];
        for (name, var) in &cfg.accounts.names {
            let until = crate::access::capped_until(&cfg, &events, name, ctx.now);
            let a = self.account(ctx, &cfg, &events, name);
            let state = match until {
                None => "ok",
                Some(_) if a.cap.starts_with("signed out") => "signed out",
                Some(_) => "capped",
            };
            let (roles, fleets) = on(&|r| r.account_list().contains(name));
            out.push(Provider {
                name: name.clone(),
                kind: "anthropic".into(),
                state: state.into(),
                until: until.filter(|t| *t != u64::MAX),
                account: Some(a),
                balance: None,
                secret: service("all", var),
                roles,
                fleets,
                lanes: count(&|l| l.account.as_deref() == Some(name)),
            });
        }
        let ds = cfg.providers.deepseek.as_ref();
        for name in keys(&cfg, &events) {
            let set = ds.is_some_and(|p| p.key_env == name);
            let last = events
                .iter()
                .rfind(|e| e.kind == "key-checked" && e.str("name") == Some(&name));
            let checked = last.map(|e| Balance {
                balance: e.str("balance").map(Into::into),
                low: e.data["low"] == true,
                hint: String::new(),
            });
            let balance = set.then(|| self.balance(ctx, &cfg)).flatten().or(checked);
            let state = match (&balance, last) {
                (_, Some(e)) if e.data["ok"] == false => "rejected",
                (Some(b), _) if b.balance.is_some() => ["ok", "low"][usize::from(b.low)],
                _ => "unchecked",
            };
            let (roles, fleets) = if set {
                on(&|r| r.harness == "dsh")
            } else {
                (vec![], vec![])
            };
            out.push(Provider {
                kind: "deepseek".into(),
                state: state.into(),
                balance,
                secret: service("all", &name),
                roles,
                fleets,
                lanes: if set { count(&|l| l.harness == "dsh") } else { 0 },
                name,
                ..Provider::default()
            });
        }
        Ok(Providers { providers: out })
    }

    /// The paired POSTs `sign-in`, `key-add`, `key-check`, `key-remove` and `use`, logged `by` the device.
    pub(super) fn provider_op(&self, ctx: &Ctx, op: &str, f: &BTreeMap<String, String>, by: &str) -> Result<String> {
        let (cfg, g) = (Config::load(&ctx.config)?, |k: &str| {
            f.get(k).map_or("", String::as_str)
        });
        let (name, r) = (g("name"), self.r);
        let key = |e: &[Event]| keys(&cfg, e).contains(name);
        match op {
            "sign-in" => crate::setup::sign_in(ctx, &cfg, r, g("account"), &format!("web {by}")),
            "use" => self.switch(ctx, &cfg, f, by),
            _ if !name_ok(name) => refused("BAD_VALUE", "a key's name is A-Z, 0-9 and _"),
            "key-add" => {
                let v = g("key").trim();
                if v.is_empty() || v.len() > 8192 || v.chars().any(char::is_control) {
                    return refused("BAD_VALUE", "a key is 1 to 8,192 bytes, no control characters");
                }
                if let Some(a) = cfg.accounts.names.iter().find(|a| a.1 == name) {
                    return refused("BAD_VALUE", format!("{name} holds account {}'s token", a.0));
                }
                let svc = service("all", name);
                r.set_secret(&svc, Some(v))?;
                anyhow::ensure!(r.secret(&svc)?.as_deref() == Some(v), "the keychain did not take {svc}");
                let d = json!({"name": name, "provider": "deepseek", "by": by, "via": "web"});
                crate::events::emit(&ctx.state, &cfg, Event::new(ctx.now, "all", None, "secret-set", d))?;
                Ok(format!("key {name} saved; {}", check(ctx, &cfg, r, name, v, by)?))
            }
            _ if !key(&shared(&ctx.state)?) => refused("NO_SUCH_KEY", format!("no DeepSeek key {name}")),
            "key-check" => match r.secret(&service("all", name))? {
                Some(v) => check(ctx, &cfg, r, name, &v, by),
                None => refused("NO_SUCH_KEY", format!("{name} is not in the store")),
            },
            "key-remove" => crate::secret::rm(ctx, &cfg, r, "all", name).map(|()| format!("key {name} removed")),
            _ => refused("BAD_VALUE", format!("no op {op}")),
        }
    }

    /// `use`: `to` (an `[accounts]` name or `deepseek`) on the roles in `scope`, as a config proposal card in `thread`
    /// that only the owner applies; with `move`, the card names the running lanes to move.
    fn switch(&self, ctx: &Ctx, cfg: &Config, f: &BTreeMap<String, String>, by: &str) -> Result<String> {
        let g = |k: &str| f.get(k).map_or("", String::as_str);
        let (to, scope, thread) = (
            g("to"),
            Some(g("scope")).filter(|s| !s.is_empty()).unwrap_or("all"),
            g("thread"),
        );
        let (new, roles) = crate::propose::switch(cfg, &std::fs::read_to_string(&ctx.config)?, scope, to)?;
        let events = shared(&ctx.state)?;
        let all = self.lanes(ctx.now, cfg, &events, &events.states).unwrap_or_default();
        let moved = all.iter().filter(|l| {
            let role = format!("{}.{}", l.fleet.as_deref().unwrap_or_default(), l.role);
            let off = if to == "deepseek" {
                l.harness != "dsh"
            } else {
                l.account.as_deref() != Some(to)
            };
            off && roles.contains(&role) && !["dead", "parked"].contains(&l.state.as_str())
        });
        let moved: Vec<_> = moved.map(|l| l.name.clone()).collect();
        let lanes = if g("move") == "1" { moved.clone() } else { vec![] };
        let d = json!({"use": {"to": to, "scope": scope, "roles": roles, "move": lanes}, "by": by, "via": "web"});
        let id = crate::propose::card(ctx, cfg, thread, &new, d)?;
        let n = moved.len();
        let how = if lanes.is_empty() {
            "keep their account until respawned"
        } else {
            "to move"
        };
        Ok(format!(
            "{id}: use {to} for {scope}, {n} running lanes {how}; the owner applies it in {thread}"
        ))
    }
}

/// The `DeepSeek` keys: `[providers.deepseek]`'s `key_env` and each added by `key-add` and not removed since.
fn keys(cfg: &Config, events: &[Event]) -> BTreeSet<String> {
    let mut k: BTreeSet<_> = cfg.providers.deepseek.iter().map(|p| p.key_env.clone()).collect();
    for e in events.iter().filter(|e| e.fleet == "all") {
        let name = e.str("name").unwrap_or_default().to_string();
        if e.kind == "secret-set" && e.str("provider") == Some("deepseek") {
            k.insert(name);
        } else if e.kind == "secret-rm" {
            k.remove(&name);
        }
    }
    k
}

/// Key `name`'s (value `v`) balance read now; logs `key-checked` (`ok`, the balance, `low`: never the key).
fn check(ctx: &Ctx, cfg: &Config, r: &dyn crate::runner::Runner, name: &str, v: &str, by: &str) -> Result<String> {
    let warn = cfg.providers.deepseek.as_ref().map_or(5.0, |p| p.warn_below);
    let b = crate::usage::read_balance(r, v, ctx.now);
    let shown = b.as_ref().map(|b| format!("{} {:.2}", b.currency, b.total));
    let low = b.is_some_and(|b| b.total < warn);
    let d = json!({"name": name, "ok": shown.is_some(), "balance": shown, "low": low, "by": by, "via": "web"});
    crate::events::emit(&ctx.state, cfg, Event::new(ctx.now, "all", None, "key-checked", d))?;
    Ok(
        shown.map_or(format!("{name}: no balance read (rejected or unreachable)"), |s| {
            format!("{name}: {s}")
        }),
    )
}
