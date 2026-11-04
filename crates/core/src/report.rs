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
    let cut = (0..=8192.min(g("html").len()))
        .rev()
        .find(|&i| g("html").is_char_boundary(i))
        .unwrap_or(0);
    let note: String = g("note").lines().next().unwrap_or_default().chars().take(200).collect();
    let parse = |k: &str| serde_json::from_str::<Value>(g(k)).unwrap_or(Value::Null);
    let (mut n, dir) = (0, root(ctx));
    std::fs::create_dir_all(&dir)?;
    let (id, at) = loop {
        n += 1;
        let id = format!("{}-{n}", ctx.now);
        match std::fs::create_dir(dir.join(&id)) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            r => break (r.map(|()| id.clone())?, dir.join(id)),
        }
    };
    let b = json!({"id": id, "ts": ctx.now, "device": device, "build": crate::VERSION, "note": note,
        "html": &g("html")[..cut], "styles": parse("styles"), "context": parse("ctx"), "png": "shot.png"});
    std::fs::write(at.join("shot.png"), png)?;
    std::fs::write(at.join("bundle.json"), serde_json::to_string_pretty(&b)? + "\n")?;
    let path = at.join("bundle.json").display().to_string();
    let fleet = cfg.fleet.keys().next().map_or("", String::as_str);
    let d = json!({"id": id, "path": path, "device": device, "note": note, "reason": format!("{note} → {path}")});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "report", d))?;
    Ok(format!("report {id} sent: {path}"))
}

/// `herdsman report [ID]`: each report's id, note and bundle path, newest first; with an id, its bundle.
pub fn show(ctx: &Ctx, out: &mut dyn Write, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        let p = root(ctx).join(id).join("bundle.json");
        ensure!(!id.contains('/') && p.exists(), "no report {id}");
        return Ok(write!(out, "{}", std::fs::read_to_string(p)?)?);
    }
    let mut all: Vec<_> = std::fs::read_dir(root(ctx))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    all.sort();
    for p in all.iter().rev().map(|p| p.join("bundle.json")) {
        let b: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_default()).unwrap_or_default();
        writeln!(
            out,
            "{}  {}  {}",
            b["id"].as_str().unwrap_or("?"),
            b["note"].as_str().unwrap_or(""),
            p.display()
        )?;
    }
    Ok(())
}
