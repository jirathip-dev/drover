use crate::Service;
use drover_core::config::{limits, Config};
use drover_core::runner::argv;
use drover_core::Ctx;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

pub(crate) fn fleets(ctx: &Ctx) -> BTreeMap<String, Notify> {
    let names = Config::load(&ctx.config).map(|c| c.fleet.into_keys().collect::<Vec<_>>());
    names
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f, Notify::new()))
        .collect()
}

impl Service {
    pub(crate) fn loops(self: &Arc<Self>) {
        for name in self.fleets.keys() {
            tokio::spawn(self.clone().fleet(name.clone()));
        }
    }

    async fn fleet(self: Arc<Self>, name: String) {
        let Some(woke) = self.fleets.get(&name) else { return };
        loop {
            timeout(Duration::from_secs(limits().fast_secs), woke.notified())
                .await
                .ok();
            let (r, exe, a) = (self.r, self.ctx.exe.clone(), argv(&["drive", &name]));
            let most = Duration::from_secs(2 * limits().pass_max_secs);
            tokio::task::spawn_blocking(move || r.run_for(&exe, &a, "", most))
                .await
                .ok();
        }
    }
}
