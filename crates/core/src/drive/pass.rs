use crate::Ctx;

/// A held pass or kick lease; only `drive` builds one, so only a pass or kick writes the flow table's events.
pub struct Pass<'a> {
    ctx: &'a Ctx,
    fleet: &'a str,
    kick: Option<u64>,
}

impl<'a> Pass<'a> {
    pub(super) fn of(ctx: &'a Ctx, fleet: &'a str, kick: Option<u64>) -> Pass<'a> {
        Pass { ctx, fleet, kick }
    }
    pub fn ctx(&self) -> &Ctx {
        self.ctx
    }
    pub fn fleet(&self) -> &str {
        self.fleet
    }
    /// The kicked issue, when this is a kick's lease.
    pub fn kick(&self) -> Option<u64> {
        self.kick
    }
}
