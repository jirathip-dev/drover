use crate::events::{Event, Json};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// The service's local socket, beside the state database.
pub fn sock(ctx: &Ctx) -> PathBuf {
    ctx.state.with_file_name("service.sock")
}

/// One request line to the running service and its one reply line; `None` when no service listens.
pub fn call(ctx: &Ctx, req: &Value) -> Option<Value> {
    let s = UnixStream::connect(sock(ctx)).ok()?;
    let t = Some(Duration::from_secs(crate::config::limits().service_call_secs));
    s.set_read_timeout(t).ok()?;
    s.set_write_timeout(t).ok()?;
    (&s).write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

/// The running service's pid.
pub fn up(ctx: &Ctx) -> Option<u64> {
    call(ctx, &json!({"ping": true}))?.at("pid").as_u64()
}

/// A kick the service drives in process at once: its pid (the spawn marker's holder).
pub fn kick(ctx: &Ctx, fleet: &str, issue: u64, why: &str, id: Option<&str>) -> Option<u32> {
    let v = call(
        ctx,
        &json!({"kick": {"fleet": fleet, "issue": issue, "why": why, "id": id}}),
    )?;
    v.at("pid").as_u64().and_then(|p| u32::try_from(p).ok())
}

/// Fleet `fleet`'s pass started by the running service's loop for it, at once or right after the one it runs; false: no service listens.
pub fn wake(ctx: &Ctx, fleet: &str) -> bool {
    call(ctx, &json!({"wake": fleet})).is_some()
}

/// The live log after `seq`, for API v1's event stream.
pub fn since(ctx: &Ctx, seq: u64) -> Result<Vec<Event>> {
    crate::db::events(&ctx.state, "archived = 0 AND seq > ?1", seq)
}
