use super::{lock, Pass, CM};
use crate::runner::{argv, Out};
use anyhow::Result;
use std::time::Duration;

impl Pass<'_> {
    pub(super) fn up(&self, m: &str) -> bool {
        if let Some(&u) = lock(&self.up).get(m) {
            return u;
        }
        if !self.probe {
            return true;
        }
        let secs = crate::config::limits().ssh_quick_secs;
        let ssh = |a: Vec<String>| self.r.run_for("ssh", &a, "", Duration::from_secs(secs));
        let late = |o: &Result<Out>| o.as_ref().is_ok_and(|o| o.code == 124);
        let connect = format!("-oConnectTimeout={secs}");
        let probe = || {
            ssh(argv(&[
                "-oBatchMode=yes",
                &connect,
                "-oControlPath=none",
                m,
                "--",
                "true",
            ]))
        };
        let mut o = ssh(argv(&[CM[1], "-O", "check", m]));
        if !o.as_ref().is_ok_and(|o| o.code == 0) {
            o = probe();
            if late(&o) {
                o = probe();
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
