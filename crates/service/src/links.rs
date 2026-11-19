use crate::Service;
use drover_core::config::Config;
use std::sync::Arc;

impl Service {
    pub(crate) fn links(self: &Arc<Self>) {
        let hosts = Config::load(&self.ctx.config).map(|c| drover_core::link::hosts(&c));
        for (name, m) in hosts.unwrap_or_default() {
            let me = self.clone();
            std::thread::spawn(move || drover_core::link::run(&me.ctx, me.r, (&name, m.as_deref()), me.clock));
        }
    }
}
