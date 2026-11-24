use super::*;

const STALL: u64 = 30 * 60;

fn asleep_rev(name: &str, spawned: u64) -> (drover::Ctx, PathBuf, String) {
    let c = setup(name);
    reconfig(&c, &format!("{CONFIG}\n[hosts.mini]\n"));
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("6f832ae");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "sendmeter-rev-12-r1", "workspace": "",
        "runtime": "shed", "worktree": rev, "branch": "rev-12-r1", "sha": head, "host": "mini", "confirmed": true});
    ev(&c, c.now - spawned, "spawn", d);
    (c, rev, head)
}

fn asleep_fake(rev: &Path, head: &str) -> Fake {
    let at = rev.display().to_string();
    fake(&[("sendmeter-rev-12-r1", "hibernated", at.as_str())])
        .reply("gh pr view issue-12", &pr(head, "pending"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "")
}

fn later(c: &drover::Ctx, s: u64) -> drover::Ctx {
    drover::Ctx {
        now: c.now + s,
        ..c.clone()
    }
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    let all = read(&c.state).unwrap();
    all.iter().filter(|e| e.kind == kind).cloned().collect()
}

fn respawns(c: &drover::Ctx) -> usize {
    let cmd = "drover spawn sendmeter 12 rev --supersede";
    let driven = kinds(c, "driven").into_iter();
    driven
        .filter(|e| e.str("step") == Some("respawn") && e.str("cmd") == Some(cmd))
        .count()
}

#[test]
fn a_hibernated_local_reviewers_verdict_at_the_head_is_recorded_in_one_pass() {
    let (c, rev, head) = asleep_rev("drive-asleep-rev", 3600);
    std::fs::write(rev.join("verdict.md"), format!("PASS\n\nReviewed head: {head}\n")).unwrap();
    let (out, steps) = drive(&c, &asleep_fake(&rev, &head));
    assert_eq!(steps, ["verdict"], "{out}");
    let v = kinds(&c, "verdict");
    let v = v.first().unwrap_or_else(|| panic!("{out}"));
    assert_eq!((v.str("verdict"), v.str("sha")), (Some("PASS"), Some(head.as_str())));
}

#[test]
fn a_hibernated_reviewer_with_no_verdict_past_the_stall_is_prompted_once_then_respawned() {
    let (c, rev, head) = asleep_rev("drive-asleep-rev-none", STALL + 600);
    let r = asleep_fake(&rev, &head);
    let (out, _) = drive(&c, &r);
    assert_eq!(kinds(&c, "verdict-prompted").len(), 1, "{out}");
    assert_eq!(sh().sent("sendmeter-rev-12-r1").len(), 1, "{out}");
    assert_eq!(respawns(&c), 0, "{out}");
    let (again, _) = drive(&later(&c, 60), &r);
    assert_eq!(kinds(&c, "verdict-prompted").len(), 1, "{again}");
    assert_eq!(sh().sent("sendmeter-rev-12-r1").len(), 1, "{again}");
    assert_eq!(respawns(&c), 0, "{again}");
    let (last, _) = drive(&later(&c, STALL + 60), &r);
    assert_eq!(respawns(&c), 1, "{last}");
    assert_eq!(kinds(&c, "verdict-prompted").len(), 1, "{last}");
}
