use super::{lock, Pass, CM};
use crate::runner::{argv, Out, Runner};
use anyhow::Result;
use std::time::Duration;

impl Pass<'_> {
    /// The fast step's poll: every host is tried afresh, never held offline by an earlier pass's shared mark.
    #[must_use]
    pub fn own(mut self) -> Self {
        if let Ok(up) = self.up.get_mut() {
            up.clear();
        }
        self
    }
    pub(super) fn up(&self, m: &str) -> bool {
        if let Some(&u) = lock(&self.up).get(m) {
            return u;
        }
        if !self.probe {
            return true;
        }
        let l = crate::config::limits();
        let late = |o: &Result<Out>| o.as_ref().is_ok_and(|o| o.code == 124);
        let mut o = ssh(self.r, &[CM[1], "-O", "check", m], l.ssh_quick_secs);
        if !o.as_ref().is_ok_and(|o| o.code == 0) {
            o = reach(self.r, m, l.ssh_quick_secs).1;
            if !o.as_ref().is_ok_and(|o| o.code != 124) {
                o = reach(self.r, m, l.ssh_connect_secs).1;
            }
        }
        if let Ok(o) = &o {
            self.key_changed(m, o);
        }
        if late(&o) {
            lock(&self.late).insert(m.into());
        }
        let u = o.is_ok_and(|o| !matches!(o.code, 124 | 255));
        self.seen(m, u);
        u
    }
}

/// The fresh handshake with machine `m` (`ssh … true`, no shared connection): `ssh_quick_secs` to connect, `secs` in all; the command as run and its answer.
pub fn reach(r: &dyn Runner, m: &str, secs: u64) -> (String, Result<Out>) {
    let connect = format!("-oConnectTimeout={}", crate::config::limits().ssh_quick_secs);
    let a = ["-oBatchMode=yes", &connect, "-oControlPath=none", m, "--", "true"];
    (format!("ssh {}", a.join(" ")), ssh(r, &a, secs))
}

fn ssh(r: &dyn Runner, a: &[&str], secs: u64) -> Result<Out> {
    r.run_for("ssh", &argv(a), "", Duration::from_secs(secs))
}

/// Why `hosts refresh` finds machine `m` offline: its probe's command, exit code and last stderr line; the shared offline mark set or cleared by it.
pub fn why_down(ctx: &crate::Ctx, r: &dyn Runner, m: &str) -> String {
    let (cmd, o) = reach(r, m, crate::config::limits().ssh_connect_secs);
    super::went(ctx, m, o.as_ref().is_ok_and(|o| o.code == 0));
    let said = o.map_or_else(
        |e| format!("{e:#}"),
        |o| {
            let tail = o.stderr.lines().map(str::trim).rfind(|l| !l.is_empty());
            format!("exit {}: {}", o.code, tail.unwrap_or("no stderr"))
        },
    );
    format!("probe `{cmd}` {said}")
}

impl Pass<'_> {
    pub(super) fn key_changed(&self, m: &str, o: &Out) {
        if !o.stderr.contains(super::KEY_CHANGED) {
            return;
        }
        let Some((ctx, cfg)) = self.log else {
            return;
        };
        let host = cfg
            .hosts
            .iter()
            .find(|h| h.1.machine.as_deref() == Some(m))
            .map_or(m, |h| h.0.as_str());
        let events = crate::events::working(&ctx.state).unwrap_or_default();
        let kinds = ["host-key-changed", "host-online"];
        let last = events
            .iter()
            .rev()
            .find(|e| e.str("host") == Some(host) && kinds.contains(&e.kind.as_str()));
        if last.is_some_and(|e| e.kind == kinds[0]) {
            return;
        }
        let why = format!("host-key-changed {host}: ssh refuses its new host key; check it, then `ssh-keygen -R {m}`");
        let d = serde_json::json!({"host": host, "reason": why});
        crate::events::emit(
            &ctx.state,
            cfg,
            crate::events::Event::new(ctx.now, "drover", None, kinds[0], d),
        )
        .ok();
    }
    pub(super) fn reach_held(&self) -> bool {
        let held: Vec<String> = lock(&self.up).iter().filter(|u| !*u.1).map(|u| u.0.clone()).collect();
        let mut back = false;
        for m in held.into_iter().filter(|m| lock(&self.rechecked).insert(m.clone())) {
            lock(&self.up).insert(m.clone(), true);
            let up = crate::session::live_on(&super::On::new(self, Some(&m))).is_ok();
            lock(&self.up).insert(m.clone(), !up);
            self.seen(&m, up);
            back |= up;
        }
        back
    }
}
