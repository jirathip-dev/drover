//! Host-load gate for `herdsman spawn` (an unreadable signal admits) and the live-agent view.
use crate::config::Limits;
pub use crate::herdr::agents;
use crate::runner::{ok, Runner};
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

/// Live agents named `name` or running inside `dir`.
pub fn live_in<'a>(agents: &'a [Value], name: &str, dir: &Path) -> Vec<&'a Value> {
    let inside = |a: &Value, k: &str| a[k].as_str().is_some_and(|c| Path::new(c).starts_with(dir));
    agents
        .iter()
        .filter(|a| a["name"] == name || inside(a, "cwd") || inside(a, "foreground_cwd"))
        .collect()
}

/// #186: never signalled, whatever their cwd.
const SPARE: &[&str] = &[crate::herdr::BIN, "herdsman", "launchd"];

/// #186: `(pid, command, cwd)` of every process `lsof` sees, but `SPARE`, pid 1, this process and its parent.
pub fn cwds(r: &dyn Runner) -> Vec<(String, String, String)> {
    let me = [1, std::process::id(), std::os::unix::process::parent_id()].map(|p| p.to_string());
    let (mut out, mut p) = (vec![], (String::new(), String::new()));
    for l in stdout(r, "lsof", &["-d", "cwd", "-F", "pcn"])
        .unwrap_or_default()
        .lines()
    {
        match l.split_at(l.len().min(1)) {
            ("p", pid) => p = (pid.into(), String::new()),
            ("c", c) => p.1 = c.into(),
            ("n", n) if !me.contains(&p.0) && !SPARE.contains(&p.1.as_str()) => {
                out.push((p.0.clone(), p.1.clone(), n.into()));
            }
            _ => {}
        }
    }
    out
}

/// #186: the processes of `ps` whose cwd is inside `dir` (an absolute path at least two levels deep).
pub fn inside(ps: &[(String, String, String)], dir: &Path) -> Vec<(String, String, String)> {
    let deep = dir.is_absolute() && dir.components().count() > 2;
    ps.iter()
        .filter(|p| deep && Path::new(&p.2).starts_with(dir))
        .cloned()
        .collect()
}

/// #186: TERM every process inside `dir`, wait 5 s, KILL those still there; the commands signalled.
pub fn reap(r: &dyn Runner, dir: &Path) -> Vec<String> {
    let first = inside(&cwds(r), dir);
    let kill = |sig: &str, ps: &[(String, String, String)]| {
        let pids = ps.iter().map(|p| p.0.clone());
        let _ = r.run(
            "kill",
            &std::iter::once(sig.to_string()).chain(pids).collect::<Vec<_>>(),
        );
    };
    if first.is_empty() {
        return vec![];
    }
    kill("-TERM", &first);
    r.sleep(5);
    let left: Vec<_> = inside(&cwds(r), dir)
        .into_iter()
        .filter(|p| first.contains(p))
        .collect();
    if !left.is_empty() {
        kill("-KILL", &left);
    }
    first.into_iter().map(|p| p.1).collect()
}

#[derive(Debug, Default, Clone)]
pub struct Host {
    /// normal | warn | critical
    pub pressure: Option<String>,
    pub swapout: Option<f64>,
    /// #168: the median of the last `WINDOW` swap-out samples (`window`); the gate reads this, not one spike.
    pub swap_median: Option<f64>,
    pub load_per_cpu: Option<f64>,
    /// #168: `vm_stat` free + inactive pages, in GB.
    pub free_mem_gb: Option<f64>,
}

fn stdout(r: &dyn Runner, prog: &str, args: &[&str]) -> Option<String> {
    ok(r, prog, args).ok()
}

/// A `vm_stat` counter (`Swapouts:`, `Pages free:`).
fn field(s: &str, name: &str) -> Option<f64> {
    let line = s.lines().find(|l| l.trim_start().starts_with(name))?;
    line.split(':').nth(1)?.trim().trim_end_matches('.').parse().ok()
}

/// Free + inactive memory in GB, from `vm_stat`'s `(page size of N bytes)` header.
fn mem_gb(s: &str) -> Option<f64> {
    let page: f64 = s
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some((field(s, "Pages free:")? + field(s, "Pages inactive:")?) * page / 1_073_741_824.0)
}

/// Sample the host (macOS: sysctl + two `vm_stat` samples 2 s apart).
pub fn sample(r: &dyn Runner) -> Host {
    let pressure = stdout(r, "sysctl", &["-n", "kern.memorystatus_vm_pressure_level"]).and_then(|s| {
        match s.trim() {
            "1" | "normal" => Some("normal"),
            "2" | "warn" => Some("warn"),
            "4" | "critical" => Some("critical"),
            _ => None,
        }
        .map(String::from)
    });
    let a = stdout(r, "vm_stat", &[]);
    let b = a.as_ref().and_then(|_| {
        r.sleep(2);
        stdout(r, "vm_stat", &[])
    });
    let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
    let swapout = field(&a, "Swapouts:")
        .zip(field(&b, "Swapouts:"))
        .map(|(a, b)| (b - a).max(0.0));
    let load = stdout(r, "sysctl", &["-n", "vm.loadavg"])
        .and_then(|s| s.split_whitespace().find_map(|w| w.parse::<f64>().ok()));
    let ncpu = stdout(r, "sysctl", &["-n", "hw.ncpu"]).and_then(|s| s.trim().parse::<f64>().ok());
    let load_per_cpu = load.zip(ncpu.filter(|n| *n > 0.0)).map(|(l, n)| l / n);
    Host {
        pressure,
        swapout,
        swap_median: None,
        load_per_cpu,
        free_mem_gb: mem_gb(&b),
    }
}

/// #168: swap-out samples kept in `<state dir>/swapouts` (≈ 10 s of serve's samples, or the last spawns').
const WINDOW: usize = 5;

/// Add `h`'s swap-out to the rolling window and set `h.swap_median` from it.
pub fn window(state: &Path, h: &mut Host) {
    let file = state.with_file_name("swapouts");
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let mut w: Vec<f64> = old.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    w.extend(h.swapout);
    let w = &w[w.len().saturating_sub(WINDOW)..];
    let _ = std::fs::write(&file, w.iter().map(f64::to_string).collect::<Vec<_>>().join(" "));
    let mut m = w.to_vec();
    m.sort_by(f64::total_cmp);
    h.swap_median = m.get(m.len() / 2).copied();
}

/// #103: free GB on `/` (`df -k`, when readable); a disk row is amber within 25 % of `min_free_gb`, red under it (#141).
pub fn free_gb(r: &dyn Runner) -> Option<f64> {
    let s = stdout(r, "df", &["-k", "/"])?;
    Some(s.lines().nth(1)?.split_whitespace().nth(3)?.parse::<f64>().ok()? / 1_048_576.0)
}

/// Err(reason) when any signal is over its limit.
pub fn admit(h: &Host, l: &Limits) -> Result<(), String> {
    let mut why = vec![];
    let pressed = h.pressure.as_deref().filter(|p| *p != "normal");
    if let Some(p) = pressed {
        why.push(format!("memory pressure {p}"));
    }
    // #168: swap refuses only when sustained (the window's median) and memory pressure is up; one spike admits.
    let median = h
        .swap_median
        .or(h.swapout)
        .filter(|s| *s > l.swapout_rate && pressed.is_some());
    if let Some(s) = median {
        why.push(format!("swap-out median {s} pages/2s > {}", l.swapout_rate));
    }
    if let Some(x) = h.load_per_cpu.filter(|x| *x > l.load_per_cpu) {
        why.push(format!("load/cpu {x:.2} > {}", l.load_per_cpu));
    }
    if why.is_empty() {
        Ok(())
    } else {
        Err(why.join("; "))
    }
}
