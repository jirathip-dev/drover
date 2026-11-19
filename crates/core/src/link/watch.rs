use super::frame::{Body, Frame, Line, Report, Seen, Verdict, Watch, Worktree, V};
use crate::runner::{ok, Runner};
use serde_json::Value;
use std::path::Path;

const VERDICT_MAX: usize = 64 * 1024;

fn head(r: &dyn Runner, wt: &str) -> Option<(String, u64)> {
    let g = ok(r, "git", &["-C", wt, "log", "-1", "--format=%H %ct"]).ok()?;
    let (h, t) = g.trim().split_once(' ')?;
    Some((h.into(), t.parse().ok()?))
}

fn verdict(wt: &str) -> Option<Verdict> {
    let p = Path::new(wt).join("verdict.md");
    let at = std::fs::metadata(&p).ok()?.modified().ok()?;
    let mut text = std::fs::read_to_string(&p).ok()?;
    let cut = (0..=VERDICT_MAX.min(text.len()))
        .rev()
        .find(|i| text.is_char_boundary(*i));
    text.truncate(cut.unwrap_or_default());
    let mtime = at.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(Verdict { mtime, text })
}

fn lines(wt: &str) -> Vec<Line> {
    let text = std::fs::read_to_string(Path::new(wt).join(".drover/outbox.jsonl")).unwrap_or_default();
    let line = |l: &str| {
        let v: Value = serde_json::from_str(l).ok()?;
        Some(Line {
            worktree: wt.into(),
            sent: v.get("ts")?.as_u64()?,
            lane: v.get("lane")?.as_str()?.into(),
            args: serde_json::from_value(v.get("args")?.clone()).ok()?,
        })
    };
    text.lines().filter_map(line).collect()
}

/// This host's whole truth for `watch`: its shed's sessions, each named worktree's facts and outbox lines.
pub fn report(r: &dyn Runner, watch: &Watch) -> Report {
    let (mut sessions, shed) = match crate::session::list::raw(r) {
        Ok(all) => (all, None),
        Err(e) => (vec![], Some(format!("{e:#}"))),
    };
    let mut lanes = vec![];
    let dead: Vec<&String> = (watch.roots.iter())
        .filter(|r| !crate::runner::answers(Path::new(r)))
        .collect();
    for wt in watch
        .worktrees
        .iter()
        .filter(|w| !dead.iter().any(|r| Path::new(w).starts_with(r)))
    {
        let head = head(r, wt);
        if head.is_none() && !Path::new(wt).exists() {
            continue;
        }
        let worktree = Worktree {
            path: wt.clone(),
            head,
            dirty: crate::spawn::dirty(r, Path::new(wt)),
            verdict: verdict(wt),
            lines: lines(wt),
        };
        let at = sessions.iter().position(|s| s.cwd.starts_with(wt));
        lanes.push(Seen {
            session: at.map(|i| sessions.remove(i)),
            worktree: Some(worktree),
        });
    }
    lanes.extend(sessions.into_iter().map(|s| Seen {
        session: Some(s),
        worktree: None,
    }));
    Report {
        build: crate::VERSION.into(),
        shed,
        lanes,
    }
}

/// This host's `Report` for `watch` as a frame of its own boot.
pub fn frame(r: &dyn Runner, watch: &Watch) -> anyhow::Result<Frame> {
    let boot = (crate::clock::nanos() ^ u128::from(crate::runner::pid())) % (1 << 64);
    let at = u64::try_from(crate::clock::nanos() / 1_000_000)?;
    let body = Body::Report(report(r, watch));
    Ok(Frame {
        v: V,
        boot: format!("{boot:016x}"),
        n: 1,
        at,
        body,
    })
}

/// `drover hosts report`: the `Watch` on `stdin` answered with one frame line on `out`; with no shed runtime set, the host's own shed (`shed connect`).
pub fn serve(r: &dyn Runner, stdin: &mut dyn std::io::Read, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let own = r.shed().is_none().then(|| {
        let sh = crate::shed::Shed {
            wire: crate::shed::Wire::Connect,
            acp: None,
            policy: "ask".into(),
        };
        crate::shed::With(r, sh)
    });
    let r: &dyn Runner = own.as_ref().map_or(r, |w| w);
    let mut w = String::new();
    stdin.read_to_string(&mut w)?;
    writeln!(
        out,
        "{}",
        serde_json::to_string(&frame(r, &serde_json::from_str(&w)?)?)?
    )?;
    Ok(())
}
