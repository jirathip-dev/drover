use super::frame::{Body, Frame, Line, Report, Seen, Verdict, Watch, Worktree, V};
use crate::runner::{ok, Runner};
use serde_json::Value;
use std::path::Path;

const VERDICT_MAX: usize = 64 * 1024;
const ACKED: &str = ".drover/outbox.acked";

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

fn acked(wt: &str) -> u64 {
    let p = Path::new(wt).join(ACKED);
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

fn ack(line: &str) {
    let Some((wt, sent)) = line.split_once('\t') else {
        return;
    };
    let Some(sent) = sent.trim().parse::<u64>().ok().filter(|s| *s > acked(wt)) else {
        return;
    };
    std::fs::write(Path::new(wt).join(ACKED), sent.to_string()).ok();
}

fn lines(wt: &str) -> Vec<Line> {
    let text = std::fs::read_to_string(Path::new(wt).join(".drover/outbox.jsonl")).unwrap_or_default();
    let done = acked(wt);
    let line = |l: &str| {
        let v: Value = serde_json::from_str(l).ok()?;
        Some(Line {
            worktree: wt.into(),
            sent: v.get("ts")?.as_u64()?,
            lane: v.get("lane")?.as_str()?.into(),
            args: serde_json::from_value(v.get("args")?.clone()).ok()?,
        })
    };
    text.lines().filter_map(line).filter(|l| l.sent > done).collect()
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

fn stamp(r: &dyn Runner, watch: &Watch) -> String {
    let seen = crate::session::list::raw(r).map(|all| {
        let one = |s: &crate::shed::proto::TerminalInfo| {
            let said = s.reported.as_ref().map(|x| x.state.clone());
            format!("{} {:?} {:?} {said:?}", s.name, s.state, s.exit)
        };
        all.iter().map(one).collect::<Vec<_>>()
    });
    let stat = |wt: &String| {
        let files = [".drover/outbox.jsonl", "verdict.md", ACKED].map(|f| {
            let m = std::fs::metadata(Path::new(wt).join(f)).ok();
            m.map(|m| (m.len(), m.modified().ok()))
        });
        format!("{wt} {files:?}")
    };
    format!("{seen:?} {:?}", watch.worktrees.iter().map(stat).collect::<Vec<_>>())
}

/// `drover hosts watch`: the `Watch` on `stdin`'s first line answered with a `Report`, then one a change and a `Beat` when idle, until a frame cannot be written or `stdin` closes. Each later line of `stdin` is a new `Watch`, or an ack: `<worktree>\t<sent>`.
pub fn watch(
    r: &dyn Runner,
    stdin: &mut (dyn std::io::Read + Send),
    out: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let mut stdin = std::io::BufReader::new(stdin);
    let mut first = String::new();
    std::io::BufRead::read_line(&mut stdin, &mut first)?;
    let mut w: Watch = serde_json::from_str(&first)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(move || {
            std::io::BufRead::lines(stdin)
                .map_while(Result::ok)
                .try_for_each(|l| tx.send(l))
        });
        let Frame { boot, body, .. } = frame(r, &w)?;
        let (mut n, mut body, mut last, mut sampled) = (0, body, stamp(r, &w), crate::clock::instant());
        loop {
            n += 1;
            let at = u64::try_from(crate::clock::nanos() / 1_000_000)?;
            let f = Frame {
                v: V,
                boot: boot.clone(),
                n,
                at,
                body,
            };
            writeln!(out, "{}", serde_json::to_string(&f)?)?;
            out.flush()?;
            let said = crate::clock::instant();
            body = loop {
                crate::clock::sleep(std::time::Duration::from_millis(w.poll_millis.max(1)));
                let mut fresh = false;
                loop {
                    match rx.try_recv() {
                        Ok(l) if l.starts_with('{') => (w, fresh) = (serde_json::from_str(&l)?, true),
                        Ok(l) => ack(&l),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
                    }
                }
                let now = stamp(r, &w);
                if fresh || now != last || sampled.elapsed().as_secs() >= w.sample_secs {
                    (last, sampled) = (now, crate::clock::instant());
                    break Body::Report(report(r, &w));
                }
                if said.elapsed().as_secs() >= w.beat_secs {
                    break Body::Beat;
                }
            };
        }
    })
}

/// `drover hosts report`: the `Watch` on `stdin` answered with one frame line on `out`.
pub fn serve(r: &dyn Runner, stdin: &mut dyn std::io::Read, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let mut w = String::new();
    stdin.read_to_string(&mut w)?;
    writeln!(
        out,
        "{}",
        serde_json::to_string(&frame(r, &serde_json::from_str(&w)?)?)?
    )?;
    Ok(())
}
