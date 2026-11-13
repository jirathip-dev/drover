//! #363: the board's report mode. A paired phone's bundle (an element's markup, key styles, the board's context, a
//! cropped PNG and a one-line note) is kept under `state/reports/<id>/`; a `report` event names its path.
use crate::config::Config;
use crate::events::{emit, Event};
use crate::Ctx;
use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

fn root(ctx: &Ctx) -> PathBuf {
    ctx.state.parent().unwrap_or(&ctx.state).join("reports")
}

/// Keep the posted bundle from `device` (`html`, `styles` and `ctx` JSON, `note`, `png` base64); the reply names its path.
pub fn save(ctx: &Ctx, cfg: &Config, f: &BTreeMap<String, String>, device: &str) -> Result<String> {
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let png = STANDARD.decode(g("png").trim_start_matches("data:image/png;base64,"))?;
    ensure!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "the screenshot is not a PNG");
    let note: String = g("note").lines().next().unwrap_or_default().chars().take(200).collect();
    let parse = |k: &str| serde_json::from_str::<Value>(g(k)).unwrap_or(Value::Null);
    std::fs::create_dir_all(root(ctx))?;
    let id = (1..100)
        .map(|n| format!("{}-{n}", ctx.now))
        .find(|id| std::fs::create_dir(root(ctx).join(id)).is_ok());
    let id = id.ok_or_else(|| anyhow::anyhow!("cannot make a report folder in {}", root(ctx).display()))?;
    let at = root(ctx).join(&id);
    let b = json!({"id": id, "ts": ctx.now, "device": device, "build": crate::VERSION, "note": note, "png": "shot.png",
        "html": &g("html")[..g("html").floor_char_boundary(8192)], "styles": parse("styles"), "context": parse("ctx")});
    std::fs::write(at.join("shot.png"), png)?;
    std::fs::write(at.join("bundle.json"), serde_json::to_string_pretty(&b)? + "\n")?;
    let path = at.join("bundle.json").display().to_string();
    let fleet = cfg.fleet.keys().next().map_or("", String::as_str);
    let d = json!({"id": id, "path": path, "device": device, "note": note, "reason": format!("{note} → {path}")});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "report", d))?;
    Ok(format!("report {id} sent: {path}"))
}

/// `drover report [ID]`: each report's id, note and bundle path, newest first; with an id, its bundle.
pub fn show(ctx: &Ctx, out: &mut dyn Write, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        let p = root(ctx).join(id).join("bundle.json");
        ensure!(!id.contains('/') && p.exists(), "no report {id}");
        return Ok(write!(out, "{}", std::fs::read_to_string(p)?)?);
    }
    let mut all: Vec<_> = std::fs::read_dir(root(ctx)).map_or(vec![], |d| d.flatten().map(|e| e.path()).collect());
    all.sort();
    for p in all.iter().rev().map(|p| p.join("bundle.json")) {
        let b: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_default()).unwrap_or_default();
        let s = |k: &str| b[k].as_str().unwrap_or_default().to_string();
        writeln!(out, "{}  {}  {}", s("id"), s("note"), p.display())?;
    }
    Ok(())
}
