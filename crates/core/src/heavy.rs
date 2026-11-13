use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

/// A lane's heavy kinds (`[fleet.<n>.heavy]`: kind → command patterns) under its host's slot limits.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heavy {
    pub kinds: BTreeMap<String, Vec<String>>,
    pub max: usize,
    pub kind_max: BTreeMap<String, usize>,
    pub wait_minutes: u64,
}

impl Heavy {
    /// The first kind with a pattern matching one of `cmd`'s commands (leading `VAR=value` words skipped).
    pub fn kind(&self, cmd: &str) -> Option<&str> {
        let segs: Vec<String> = (cmd.split(['\n', ';', '|', '&', '(', ')']))
            .map(|s| {
                let w: Vec<&str> = s.split_whitespace().collect();
                let at = w.iter().position(|w| !w.contains('=') || w.starts_with('-'));
                at.map_or(String::new(), |i| w[i..].join(" "))
            })
            .collect();
        let hit = |p: &String| {
            segs.iter()
                .any(|s| crate::config::glob(p.replace('*', "**").as_bytes(), s.as_bytes()))
        };
        self.kinds
            .iter()
            .find(|(_, ps)| ps.iter().any(hit))
            .map(|(k, _)| k.as_str())
    }

    /// `cmd` as a shell line that runs it under `exe heavy`, holding a `kind` slot.
    pub fn wrap(&self, exe: &str, kind: &str, cmd: &str) -> String {
        let mut a = [
            exe,
            "heavy",
            kind,
            "--max",
            &self.max.to_string(),
            "--wait",
            &self.wait_minutes.to_string(),
        ]
        .map(String::from)
        .to_vec();
        if let Some(m) = self.kind_max.get(kind) {
            a.extend(["--kind-max".into(), m.to_string()]);
        }
        a.extend(["--".into(), cmd.into()]);
        let q = |w: &String| format!("'{}'", w.replace('\'', r"'\''"));
        a.iter().map(q).collect::<Vec<_>>().join(" ")
    }
}

/// One command in a host's line, as its file in the slot folder records it (for `drover top`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ticket {
    pub lane: String,
    pub kind: String,
    pub cmd: String,
    /// When it joined the line.
    pub since: u64,
    pub running: bool,
    pub kind_max: Option<usize>,
}

/// Every live command in the slot folder `dir`, in line order; a dead holder's file (its lock gone) is removed.
pub fn line(dir: &Path) -> Vec<(String, Ticket)> {
    let mut all: Vec<(String, Ticket)> = (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| {
            let f = File::open(e.path()).ok()?;
            if f.try_lock().is_ok() {
                let _ = std::fs::remove_file(e.path());
                return None;
            }
            let t = serde_json::from_reader(&f).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), t))
        })
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

fn admitted(dir: &Path, me: &str, max: usize) -> bool {
    let all = line(dir);
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, t) in all.iter().filter(|(_, t)| t.running) {
        *kinds.entry(&t.kind).or_default() += 1;
    }
    let mut used: usize = kinds.values().sum();
    for (n, t) in all.iter().filter(|(_, t)| !t.running) {
        let k = kinds.entry(&t.kind).or_default();
        if used < max && t.kind_max.is_none_or(|m| *k < m) {
            if n == me {
                return true;
            }
            used += 1;
            *k += 1;
        }
    }
    false
}

/// Wait in `dir`'s line as `me` (past `wait_minutes`: `late`, then run anyway), run its command, free the slot on exit.
pub fn run(
    r: &dyn crate::runner::Runner,
    dir: &Path,
    mut me: Ticket,
    max: usize,
    wait_minutes: u64,
    late: impl FnOnce(&Ticket),
) -> Result<i32> {
    std::fs::create_dir_all(dir)?;
    let guard = File::create(dir.join(".lock"))?;
    guard.lock()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let name = format!("{nanos:020}-{}.json", std::process::id());
    let path = dir.join(&name);
    let mut mine = File::create_new(&path)?;
    mine.lock()?;
    let save = |f: &mut File, t: &Ticket| -> Result<()> {
        f.set_len(0)?;
        f.rewind()?;
        Ok(f.write_all(&serde_json::to_vec(t)?)?)
    };
    save(&mut mine, &me)?;
    guard.unlock()?;
    let timed_out = loop {
        guard.lock()?;
        let go = admitted(dir, &name, max);
        let timed_out = !go && crate::now() >= me.since + wait_minutes * 60;
        if go || timed_out {
            me.running = true;
            save(&mut mine, &me)?;
        }
        guard.unlock()?;
        if go || timed_out {
            break timed_out;
        }
        std::thread::sleep(std::time::Duration::from_millis(crate::config::limits().poll_millis));
    };
    if timed_out {
        late(&me);
    }
    let code = r.attached("bash", &["-c".into(), me.cmd.clone()]);
    let _ = std::fs::remove_file(&path);
    code
}
