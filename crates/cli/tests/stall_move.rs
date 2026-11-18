mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, read, Event};
use drover::hosts::place;
use serde_json::json;

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";
const WT: &str = "/home/me/.local/share/drover/work/sendmeter/issue-12";
const IMPL: &str = "sendmeter-impl-12";

fn config(extra: &str) -> String {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!("{base}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n{extra}")
}

fn setup(name: &str, extra: &str, bazzite_up: bool, seen: u64) -> drover::Ctx {
    let c = ctx(&tmpdir(name), &config(extra));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64", "xcode"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux", "arch:x86_64", "docker"], "home": "/home/me", "probed": NOW,
                    "seen": NOW - seen, "up": bazzite_up, "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

fn fake(mini: f64, remote: f64) -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {} 1 1 }}", mini * 4.0))
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            &format!("{SSH}'cat' '/proc/loadavg'"),
            &format!("{} 1 1 1/9 42", remote * 4.0),
        )
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
}

fn two<'a>(f: &'a Fake, bazzite: &'a FakeShed) -> OverSsh<'a> {
    OverSsh {
        f,
        hosts: vec![("bazzite", bazzite)],
    }
}

fn ev(c: &drover::Ctx, ts: u64, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn remote_spawn(c: &drover::Ctx, ts: u64) {
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(c, ts, 12, "spawn", d);
}

fn placed(c: &drover::Ctx, r: &Fake, issue: u64, role: &str, force: Option<&str>) -> Result<String, String> {
    let cfg = Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let (mini, bazzite) = (FakeShed::new(), FakeShed::new());
    match place(
        c,
        &cfg,
        &mini.on(&two(r, &bazzite)),
        &events,
        ("sendmeter", issue, role),
        (force, &|_| None),
    ) {
        Ok(p) => {
            let p = p.unwrap();
            p.refusal
                .map_or(Ok(p.name), |(code, why)| Err(format!("{code}: {why}")))
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

fn busy_wt(f: Fake, status: &str, ahead: &str) -> Fake {
    f.reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{WT}' 'status'"),
            status,
        )
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{WT}' 'rev-list'"),
            ahead,
        )
}

#[test]
fn a_busy_last_host_whose_worktree_holds_only_untracked_scratch_yields_to_a_free_host() {
    let c = setup("hosts-scratch-983", "", true, 0);
    remote_spawn(&c, NOW - 60);
    let scratch = busy_wt(fake(1.0, 4.0), "?? body.md\n?? notes.txt\n", "0\n");
    let bazzite = FakeShed::new();
    let ssh = two(&scratch, &bazzite);
    let on = drover::hosts::On::new(&ssh, Some("bazzite"));
    assert!(
        !drover::hosts::unsaved(&on, std::path::Path::new(WT)),
        "scratch never pins"
    );
    assert_eq!(placed(&c, &scratch, 12, "impl", None).as_deref(), Ok("mini"));
    let tracked = busy_wt(fake(1.0, 4.0), " M src/lib.rs\n?? body.md\n", "0\n");
    let e = placed(&c, &tracked, 12, "impl", None).unwrap_err();
    assert!(
        e.starts_with("HOST_BUSY: bazzite") && e.ends_with("; stays on bazzite: its worktree there has unpushed work"),
        "{e}"
    );
}

fn load_wait(c: &drover::Ctx) {
    remote_spawn(c, NOW - 5 * 3600);
    ev(
        c,
        NOW - 3 * 3600,
        12,
        "verdict",
        json!({"verdict": "FAIL", "sha": "92aa67b", "form": true}),
    );
    for t in [NOW - 2 * 3600 - 60, NOW - 3600] {
        let why = "HOST_BUSY: bazzite: load/cpu 4.00 > 2.5; stays on bazzite: its worktree there has unpushed work";
        let d = json!({"step": "respawn", "cmd": "drover spawn sendmeter 12 impl", "result": format!("REFUSED {why}"), "code": "HOST_BUSY"});
        ev(c, t, 12, "driven", d);
    }
}

fn slo_pass(c: &drover::Ctx, r: &Fake) {
    let cfg = Config::load(&c.config).unwrap();
    let (mini, bazzite) = (FakeShed::new(), FakeShed::new());
    let ssh = two(r, &bazzite);
    let on = mini.on(&ssh);
    drover::slo::pass(c, &cfg, (&on, &on), &mut vec![], &["sendmeter"]).unwrap();
}

#[test]
fn a_fix_spawn_held_on_its_busy_pinned_host_past_the_stall_window_moves_with_its_wip_saved() {
    let c = setup("hosts-stall-move-1025", "", true, 0);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    load_wait(&c);
    let busy = || busy_wt(fake(1.0, 4.0), "", "1\n");
    let e = placed(&c, &busy(), 12, "impl", None).unwrap_err();
    assert!(e.starts_with("HOST_BUSY: bazzite"), "pinned before the pass: {e}");
    slo_pass(&c, &busy());
    let moved = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "stall-move");
    let moved = moved.expect("a stall-move event");
    assert_eq!(
        (moved.issue, moved.str("from"), moved.str("by")),
        (Some(12), Some("bazzite"), Some("drover"))
    );
    assert!(moved.str("reason").unwrap().contains("HOST_BUSY"), "{moved:?}");
    let r = busy()
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(&format!("{SSH}'git'"), "")
        .reply("git ", "");
    let (sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let (code, out) = run(
        &c,
        &two(&r, &bazzite),
        &["spawn", "sendmeter", "12", "impl", "--supersede"],
    );
    let calls = r.calls.borrow().clone();
    assert_eq!(code, 0, "{out} {calls:?}");
    assert!(sh.called("run", IMPL), "{:?}", sh.methods());
    let push = calls
        .iter()
        .position(|l| l.starts_with(&format!("{SSH}'git' '--no-replace-objects' '-C' '{WT}' 'push'")));
    let add = calls
        .iter()
        .position(|l| l.starts_with("git -C") && l.contains(" worktree add "));
    assert!(
        push.is_some() && push < add,
        "wip saved before the new worktree: {calls:?}"
    );
    let e = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "spawn")
        .unwrap();
    assert_eq!(e.str("host"), Some("mini"));
}

#[test]
fn a_load_wait_past_the_stall_window_with_no_free_host_is_a_stuck_card_never_the_busy_host() {
    let c = setup("hosts-stall-nofree", "", true, 0);
    load_wait(&c);
    let busy = || busy_wt(fake(4.0, 4.0), "", "1\n");
    slo_pass(&c, &busy());
    let e = placed(&c, &busy(), 12, "impl", None).unwrap_err();
    assert!(e.starts_with("HOST_BUSY: mini"), "never back on the busy host: {e}");
    slo_pass(
        &drover::Ctx {
            now: NOW + 301,
            ..c.clone()
        },
        &busy(),
    );
    let ev = read(&c.state).unwrap();
    let stuck = drover::slo::open_stuck(&ev);
    let mine = stuck
        .get("sendmeter")
        .into_iter()
        .flatten()
        .find(|e| e.issue == Some(12));
    let mine = mine.unwrap_or_else(|| panic!("a stuck card for #12: {stuck:?}"));
    assert!(mine.str("reason").unwrap().contains("bazzite"), "{mine:?}");
}
