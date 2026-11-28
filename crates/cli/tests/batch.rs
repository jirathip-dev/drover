mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const REPO: &str = "jirathip-dev/sendmeter";

struct Batch {
    _d: std::path::PathBuf,
    c: drover::Ctx,
    local: String,
    wt: String,
}

fn head(n: u64) -> String {
    format!("{n:0>40}")
}

fn fleet(name: &str, extra: &str, ready: &[u64]) -> Batch {
    let d = tmpdir(name);
    let local = d.join("local");
    std::fs::create_dir_all(&local).unwrap();
    let local = local.to_string_lossy().to_string();
    let cfg = CONFIG.replace("/tmp/does-not-matter/sendmeter", &local).replace(
        "integration = \"staging\"   # never main",
        &format!("integration = \"staging\"\npremerge = [\"make lint\"]\n{extra}"),
    );
    let c = ctx(&d, &cfg);
    for (i, &n) in ready.iter().enumerate() {
        let ts = 10 + u64::try_from(i).unwrap();
        let spawn = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": format!("issue-{n}")});
        append(&c.state, &Event::new(ts, "sendmeter", Some(n), "spawn", spawn)).unwrap();
        let v = json!({"verdict": "PASS", "sha": head(n), "round": 1});
        append(&c.state, &Event::new(100 + ts, "sendmeter", Some(n), "verdict", v)).unwrap();
    }
    let wt = d.join("state/premerge-sendmeter").to_string_lossy().to_string();
    Batch { _d: d, c, local, wt }
}

fn github(b: &Batch, ready: &[u64], tips: &[String]) -> Fake {
    let green = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let mut f = Fake::default();
    for t in tips {
        f = f.once(&format!("git -C {} rev-parse origin/staging", b.local), 0, t);
    }
    for &n in ready {
        let (num, h) = (1900 + n, head(n));
        let pr = json!({"number": num, "state": "OPEN", "baseRefName": "staging", "headRefOid": h, "statusCheckRollup": green});
        f = f
            .reply(&format!("gh pr view issue-{n} "), &pr.to_string())
            .reply(&format!("gh api repos/{REPO}/branches/issue-{n}"), &h)
            .reply(&format!("gh pr merge {num} "), "")
            .reply(
                &format!("gh pr view {num} -R {REPO} --json headRefOid,mergeable"),
                &json!({"headRefOid": h, "mergeable": "MERGEABLE"}).to_string(),
            )
            .reply(
                &format!("gh pr view {num} -R {REPO} --json mergeCommit"),
                &json!({"mergeCommit": {"oid": format!("m{n}")}}).to_string(),
            )
            .reply(&format!("git -C {} merge --no-ff --no-edit {h}", b.wt), "")
            .reply(&format!("git -C {} rev-parse m{n}^{{tree}}", b.local), &format!("t{n}"));
    }
    for &n in ready {
        f = f.once(
            &format!("git -C {} rev-parse HEAD HEAD^{{tree}}", b.wt),
            0,
            &format!("c{n}\nt{n}"),
        );
    }
    f.reply("gh issue", "").reply("git -C ", "")
}

fn tips(landed: &[u64], recheck: bool) -> Vec<String> {
    let mut t = vec!["base1".to_string()];
    if recheck {
        t.push("base1".into());
    }
    for (i, n) in landed.iter().enumerate() {
        t.push(format!("m{n}"));
        if i + 1 < landed.len() {
            t.push(format!("m{n}"));
        }
    }
    t
}

fn merged(c: &drover::Ctx) -> Vec<u64> {
    let ev = read(&c.state).unwrap();
    ev.iter()
        .filter(|e| e.kind == "merged")
        .filter_map(|e| e.issue)
        .collect()
}

fn merges(r: &Fake) -> Vec<String> {
    let calls = r.calls.borrow();
    let m = calls.iter().filter(|c| c.starts_with("gh pr merge "));
    m.map(|c| {
        let w: Vec<&str> = c.split(' ').collect();
        format!(
            "{} {}",
            w[3],
            w[w.iter().position(|x| *x == "--match-head-commit").unwrap() + 1]
        )
    })
    .collect()
}

fn landed(c: &drover::Ctx) -> serde_json::Value {
    let ev = read(&c.state).unwrap();
    json!(
        ev.iter()
            .rfind(|e| e.kind == "batch-landed")
            .expect("batch-landed")
            .data
    )
}

fn state(c: &drover::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

#[test]
fn a_green_batch_runs_the_gate_once_and_merges_each_pr_at_its_own_head() {
    let ready = [12, 15, 19];
    let b = fleet("batch-green", "batch_max = 4", &ready);
    let r = github(&b, &ready, &tips(&ready, true)).reply("sh -c", "ok\n");
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("sh -c"), 1, "{:#?}", r.calls.borrow());
    assert_eq!(merges(&r), [12, 15, 19].map(|n| format!("19{n} {}", head(n))));
    assert_eq!(merged(&b.c), [12, 15, 19]);
    let ev = read(&b.c.state).unwrap();
    let m: Vec<_> = ev.iter().filter(|e| e.kind == "merged").collect();
    assert!(m.iter().all(|e| e.str("batch") == Some("t19")), "{m:?}");
    assert!(m[0].val("premerge_secs").is_u64() && !m[1].data.contains_key("premerge_secs"));
    assert_eq!(landed(&b.c)["outcome"], "landed");
    assert_eq!(landed(&b.c)["landed"], json!([12, 15, 19]));
}

#[test]
fn a_red_stack_green_on_its_retry_lands_whole_and_is_logged_flaky() {
    let ready = [12, 15, 19];
    let b = fleet("batch-flaky", "batch_max = 4", &ready);
    let r = github(&b, &ready, &tips(&ready, true))
        .once("sh -c", 1, "lint: error: timed out\n")
        .reply("sh -c", "ok\n");
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("sh -c"), 2);
    assert_eq!(merged(&b.c), [12, 15, 19]);
    assert_eq!(landed(&b.c)["outcome"], "flaky");
    assert!(read(&b.c.state).unwrap().iter().all(|e| e.kind != "premerge-failed"));
}

#[test]
fn a_bisect_charges_the_one_culprit_among_eight_in_five_runs_and_lands_those_under_it() {
    let ready: Vec<u64> = (12..20).collect();
    let b = fleet("batch-bisect", "batch_max = 8", &ready);
    let r = github(&b, &ready, &tips(&[12, 13, 14, 15, 16], false))
        .once("sh -c", 1, "lint: error: unused import\n")
        .once("sh -c", 1, "lint: error: unused import\n")
        .once("sh -c", 0, "ok\n")
        .once("sh -c", 1, "lint: error: unused import\n")
        .once("sh -c", 0, "ok\n");
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("sh -c"), 5, "{:#?}", r.calls.borrow());
    let resets: Vec<String> = (r.calls.borrow().iter())
        .filter_map(|c| {
            c.strip_prefix(&format!("git -C {} reset -q --hard c", b.wt))
                .map(String::from)
        })
        .collect();
    assert_eq!(resets, ["19", "19", "15", "17", "16"], "top, retry, then the halves");
    assert_eq!(merged(&b.c), [12, 13, 14, 15, 16]);
    assert_eq!(merges(&r).len(), 5);
    let ev = read(&b.c.state).unwrap();
    let p = ev.iter().find(|e| e.kind == "premerge-failed").unwrap();
    assert_eq!(p.issue, Some(17));
    let v = ev.iter().rfind(|e| e.kind == "verdict").unwrap();
    assert_eq!((v.issue, v.str("sha")), (Some(17), Some(head(17).as_str())));
    assert!(v.str("body").unwrap().contains("on #12, #13, #14, #15, #16"), "{v:?}");
    assert_eq!(state(&b.c, 17), "fix(1)");
    assert_eq!((state(&b.c, 18), state(&b.c, 19)), ("ready".into(), "ready".into()));
    let l = landed(&b.c);
    assert_eq!(
        (l["outcome"].clone(), l["culprit"].clone()),
        (json!("charged"), json!(17))
    );
    assert_eq!(l["left"], json!([18, 19]));
}

#[test]
fn a_serial_pr_and_a_migration_pr_each_merge_alone() {
    let db = "batch_max = 4\n\n[[fleet.sendmeter.serial]]\nname = \"db\"\npaths = [\"db/migrations/**\"]\n";
    let ready = [12, 15, 19];
    let b = fleet("batch-alone", db, &ready);
    let q = json!({"op": "add", "issues": [12], "serial": ["db"]});
    append(&b.c.state, &Event::new(5, "sendmeter", None, "queue", q)).unwrap();
    let joined = json!({"group": "db", "by": "diff"});
    append(
        &b.c.state,
        &Event::new(200, "sendmeter", Some(15), "serial-joined", joined),
    )
    .unwrap();
    let files = format!("gh api repos/{REPO}/pulls/1919/files");
    let r = github(&b, &ready, &[])
        .reply(&files, "src/a.rs\n")
        .reply("sh -c", "ok\n");
    for n in [19, 12, 15] {
        let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", &n.to_string()]);
        assert_eq!(exit, 0, "#{n}: {out}");
        if n != 19 {
            assert!(
                out.contains(&format!("batch: #{n} merges alone: serial group db")),
                "{out}"
            );
        }
    }
    assert_eq!(merged(&b.c), [19, 12, 15]);
    assert_eq!(r.count("sh -c"), 3);
    let ev = read(&b.c.state).unwrap();
    assert!(ev.iter().all(|e| !e.kind.starts_with("batch-")), "each merged alone");
}

#[test]
fn a_rider_that_conflicts_is_left_ready_with_no_event() {
    let ready = [12, 15, 19];
    let b = fleet("batch-conflict", "batch_max = 4", &ready);
    let r = github(&b, &[12, 19], &tips(&[12, 19], true))
        .once(&format!("git -C {} merge --no-ff --no-edit {}", b.wt, head(15)), 1, "CONFLICT")
        .reply(
            "gh pr view issue-15 ",
            &json!({"number": 1915, "state": "OPEN", "baseRefName": "staging", "headRefOid": head(15),
                    "statusCheckRollup": [{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
            .to_string(),
        )
        .reply(&format!("gh api repos/{REPO}/branches/issue-15"), &head(15))
        .reply("sh -c", "ok\n");
    let on15 = |c: &drover::Ctx| read(&c.state).unwrap().iter().filter(|e| e.issue == Some(15)).count();
    let before = on15(&b.c);
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(on15(&b.c), before, "nothing logged on #15");
    assert!(r.called(&format!("git -C {} merge --abort", b.wt)));
    assert_eq!(merged(&b.c), [12, 19]);
    assert_eq!(state(&b.c, 15), "ready");
    let ev = read(&b.c.state).unwrap();
    let p = ev.iter().find(|e| e.kind == "batch-planned").unwrap();
    assert_eq!(p.val("dropped"), &json!([{"issue": 15, "why": "conflict"}]));
}

#[test]
fn a_head_that_moved_stops_the_landing_and_the_rest_stay_ready() {
    let ready = [12, 15, 19];
    let b = fleet("batch-moved", "batch_max = 4", &ready);
    let r = github(&b, &ready, &tips(&ready, true))
        .once(
            &format!("gh pr view 1915 -R {REPO} --json headRefOid,mergeable"),
            0,
            &json!({"headRefOid": "h15b", "mergeable": "MERGEABLE"}).to_string(),
        )
        .reply("sh -c", "ok\n");
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(merged(&b.c), [12]);
    assert_eq!((state(&b.c, 15), state(&b.c, 19)), ("ready".into(), "ready".into()));
    let l = landed(&b.c);
    assert_eq!(
        (l["outcome"].clone(), l["why"].clone()),
        (json!("stopped"), json!("head moved"))
    );
}

#[test]
fn with_batch_max_unset_merge_logs_no_batch_event() {
    let ready = [12, 15];
    let b = fleet("batch-off", "", &ready);
    let r = github(&b, &ready, &tips(&[12], true)).reply("sh -c", "ok\n");
    let (exit, out) = run(&b.c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(merges(&r), [format!("1912 {}", head(12))]);
    assert_eq!(state(&b.c, 15), "ready");
    assert!(read(&b.c.state).unwrap().iter().all(|e| !e.kind.starts_with("batch-")));
}

#[test]
fn batch_max_zero_or_with_fast_first_does_not_load() {
    let land = "\n[fleet.sendmeter.land]\nmode = \"fast-first\"\nfast = [\"build\"]\n";
    for (max, tail, want) in [
        (0, "", "batch_max is at least 1"),
        (2, land, "batch_max and fast-first landing together"),
    ] {
        let toml = CONFIG.replace(
            "merge_label = \"off\"",
            &format!("merge_label = \"off\"\nbatch_max = {max}"),
        ) + tail;
        let err = drover::config::Config::parse(&toml).unwrap_err();
        assert!(format!("{err:#}").contains(want), "{err:#}");
    }
}
