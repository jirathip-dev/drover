use super::{Kind, Request, ToolCall};
use crate::hook::Call;
use serde_json::Value;
use std::path::Path;

const COMMANDS: [&str; 3] = ["command", "cmd", "script"];
const PATHS: [&str; 6] = [
    "file_path",
    "path",
    "notebook_path",
    "filePath",
    "absolute_path",
    "destination",
];
const PATCHES: [&str; 2] = ["patch", "input"];

/// The rules' call for a judge request's ACP `toolCall`; Err names why it cannot be read.
pub fn of(req: &Request) -> Result<Call, String> {
    let t = req.tool_call.as_ref().ok_or("the request has no toolCall")?;
    let raw = t.raw_input.as_ref().unwrap_or(&Value::Null);
    let cwd = req.cwd.to_string_lossy();
    match t.kind.unwrap_or(Kind::Other) {
        Kind::Read | Kind::Search | Kind::Think | Kind::Fetch => Ok(Call::Free),
        Kind::SwitchMode => Ok(Call::Mode),
        Kind::Execute => shell(raw, &cwd).ok_or_else(|| "an execute call with no readable command".into()),
        Kind::Edit | Kind::Delete | Kind::Move => write(t, raw).ok_or_else(|| "a write with no path".into()),
        Kind::Other if COMMANDS.iter().any(|k| raw.get(k).is_some()) => {
            shell(raw, &cwd).ok_or_else(|| "a call with an unreadable command".into())
        }
        Kind::Other => Ok(write(t, raw).unwrap_or(Call::Free)),
    }
}

fn shell(raw: &Value, cwd: &str) -> Option<Call> {
    let cmd = match COMMANDS.iter().find_map(|k| raw.get(k))? {
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            let w: Vec<&str> = a.iter().map(Value::as_str).collect::<Option<_>>()?;
            match w.as_slice() {
                [sh, "-c", script, ..] if sh.ends_with("sh") => (*script).to_string(),
                _ => w.join(" "),
            }
        }
        _ => return None,
    };
    let own = ["cwd", "workdir"].iter().find_map(|k| raw.get(k)?.as_str());
    let cwd = own.filter(|d| Path::new(d).is_absolute()).unwrap_or(cwd);
    (!cmd.trim().is_empty()).then(|| Call::Shell { cmd, cwd: cwd.into() })
}

fn write(t: &ToolCall, raw: &Value) -> Option<Call> {
    let mut files: Vec<String> = t
        .locations
        .iter()
        .map(|l| l.path.to_string_lossy().into_owned())
        .collect();
    files.extend(PATHS.iter().filter_map(|k| raw.get(k)?.as_str()).map(String::from));
    let patch = PATCHES
        .iter()
        .filter_map(|k| raw.get(k)?.as_str())
        .find(|p| p.starts_with("*** "));
    files.extend(patch.map(crate::hook::patched).unwrap_or_default());
    files.retain(|f| !f.is_empty());
    files.dedup();
    (!files.is_empty()).then_some(Call::Write { files })
}
