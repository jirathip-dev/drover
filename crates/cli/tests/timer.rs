//! #98 `herdsman timer|tick|setup` and the Services rows: golden job files, idempotent setup, adopt, tick-failed.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;
use std::path::Path;

const HERDR: &str = "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n";
const MAC: bool = cfg!(target_os = "macos");

fn host(r: Fake) -> Fake {
    r.reply("herdr status", HERDR)
        .reply("herdr agent list", NO_AGENTS)
        .reply("which hermes", "/opt/hermes/bin/hermes\n")
        .reply("which gh", "/opt/homebrew/bin/gh\n")
        .reply("which herdr", "/opt/homebrew/bin/herdr\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.00 1.00 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "8\n")
        .reply("vm_stat", "Swapouts: 5.\n")
}

/// The tick and serve jobs, launchd and systemd, against tests/golden/timer/ (`BLESS=1` rewrites them).
#[test]
fn job_files_match_their_goldens() {
    let d = tmpdir("timer-golden");
    let c = ctx(&d, CONFIG);
    let path = "/opt/hermes/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/timer");
    for mac in [true, false] {
        for (label, args, every) in [
            ("dev.herdsman.tick", &["/x/herdsman", "tick"][..], 300),
            (
                "dev.herdsman.serve",
                &["/x/herdsman", "serve", "--addr", "127.0.0.1:7777"][..],
                0,
            ),
        ] {
            for (p, text) in herdsman::timer::files(&c, label, args, path, every, mac) {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let text = text.replace(&d.display().to_string(), "<dir>");
                if std::env::var_os("BLESS").is_some() {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(&name), &text).unwrap();
                }
                assert_eq!(text, std::fs::read_to_string(dir.join(&name)).unwrap(), "{name}");
            }
        }
    }
}

fn job(c: &herdsman::Ctx, label: &str) -> std::path::PathBuf {
    match MAC {
        true => c.home.join(format!("Library/LaunchAgents/{label}.plist")),
        false => c.home.join(format!(".config/systemd/user/{label}.service")),
    }
}

#[test]
fn install_no_load_writes_without_loading_and_remove_deletes() {
    let c = ctx(&tmpdir("timer-install"), CONFIG);
    let r = host(Fake::default());
    let (code, out) = run(&c, &r, &["timer", "install", "--no-load"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("✓ wrote dev.herdsman.tick; load it: "), "{out}");
    let text = std::fs::read_to_string(job(&c, "dev.herdsman.tick")).unwrap();
    // PATH: the harness, gh, herdr and herdsman dirs, then the system dirs with /usr/sbin:/sbin.
    let path = "/opt/hermes/bin:/opt/homebrew/bin:/opt/herdsman/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    assert!(text.contains(path) && text.contains("tick"), "{text}");
    assert!(!r.called("launchctl bootstrap") && !r.called("systemctl --user enable"));
    assert_eq!(
        run(&c, &r, &["timer", "install", "--no-load"]).1,
        "= already dev.herdsman.tick\n= already dev.herdsman.fast\n"
    );
    // #212: the fast job runs `drive --fast` every `fast_secs` (60); systemd keeps the interval in its .timer.
    let fast = std::fs::read_to_string(job(&c, "dev.herdsman.fast")).unwrap();
    let every = match MAC {
        true => fast.clone(),
        false => std::fs::read_to_string(c.home.join(".config/systemd/user/dev.herdsman.fast.timer")).unwrap(),
    };
    assert!(fast.contains("--fast") && every.contains("60"), "{fast}\n{every}");

    let (code, out) = run(&c, &r, &["timer", "remove"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "✓ removed dev.herdsman.tick\n✓ removed dev.herdsman.fast\n")
    );
    assert!(!job(&c, "dev.herdsman.tick").exists() && !job(&c, "dev.herdsman.fast").exists());
    assert!(r.called("launchctl bootout") || r.called("systemctl --user disable --now dev.herdsman.tick.timer"));
    assert_eq!(
        run(&c, &r, &["timer", "remove"]).1,
        "= no dev.herdsman.tick job\n= no dev.herdsman.fast job\n"
    );
}

#[test]
fn install_loads_and_prints_the_command() {
    let c = ctx(&tmpdir("timer-load"), CONFIG);
    let r = host(Fake::default());
    let (code, out) = run(&c, &r, &["timer", "install"]);
    let cmd = if MAC {
        "launchctl bootstrap gui/"
    } else {
        "systemctl --user enable --now dev.herdsman.tick.timer"
    };
    assert_eq!(code, 1, "the fake has no {cmd}: {out}");
    assert!(
        out.starts_with("✗ dev.herdsman.tick: ") && out.contains(&format!("→ {cmd}")),
        "{out}"
    );
    // launchd's bootstrap prefix covers both jobs; systemd enables each job's own .timer.
    let fast = if MAC {
        cmd
    } else {
        "systemctl --user enable --now dev.herdsman.fast.timer"
    };
    let r = host(Fake::default()).reply(cmd, "").reply(fast, "");
    let (code, out) = run(&c, &r, &["timer", "install"]);
    assert_eq!((code, out.starts_with(&format!("✓ {cmd}"))), (0, true), "{out}");
    let fast_line = out.lines().nth(1).unwrap_or_default();
    assert!(fast_line.starts_with(&format!("✓ {fast}")), "{out}");
    assert!(fast_line.contains("dev.herdsman.fast"), "{out}");
    let (_, out) = run(&c, &r, &["timer", "status"]);
    assert_eq!(out, "dev.herdsman.tick: not loaded · tick ran never\n");
}

#[test]
fn tick_runs_check_then_drive_and_exits_zero_on_refusals() {
    let c = ctx(&tmpdir("timer-tick"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    // Memory pressure critical: the start is refused HOST_BUSY; the tick still exits 0.
    let r = host(Fake::default().once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n")).reply("git -C", "");
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter #12 start: REFUSED HOST_BUSY"), "{out}");
    for w in ["check.at", "drive.at"] {
        assert_eq!(
            std::fs::read_to_string(c.state.with_file_name(w)).unwrap(),
            "1000000",
            "{w}"
        );
    }
    assert!(!read(&c.state).unwrap().iter().any(|e| e.kind == "tick-failed"));
}

#[test]
fn tick_failed_is_logged_at_most_once_an_hour() {
    let d = tmpdir("timer-fail");
    let c = ctx(&d, "[fleet.x\nbroken");
    let fails = |c: &herdsman::Ctx| {
        let (code, out) = run(c, &host(Fake::default()), &["tick"]);
        assert_eq!(code, 1, "{out}");
        assert!(out.starts_with("tick failed: "), "{out}");
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "tick-failed")
            .count()
    };
    assert_eq!(fails(&c), 1);
    assert_eq!(
        fails(&herdsman::Ctx {
            now: c.now + 300,
            ..c.clone()
        }),
        1
    );
    assert_eq!(
        fails(&herdsman::Ctx {
            now: c.now + 3601,
            ..c.clone()
        }),
        2
    );
    // herdr missing is a real error too.
    let c = ctx(&tmpdir("timer-noherdr"), CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["tick"]);
    assert_eq!(code, 1, "{out}");
    let e = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "tick-failed")
        .unwrap();
    assert!(e.str("reason").unwrap().starts_with("herdr: "), "{e:?}");
}

/// The five step lines `setup` prints before doctor's rows.
fn steps(out: &str) -> Vec<&str> {
    out.lines().take(5).collect()
}

#[test]
fn setup_twice_is_all_already_and_tailscale_absent_is_skipped() {
    let cfg = format!("{CONFIG}\n[serve]\ntailscale_https_port = 7443\n");
    let c = ctx(&tmpdir("timer-setup"), &cfg);
    let probe = if MAC {
        "launchctl print"
    } else {
        "systemctl --user is-active"
    };
    let load = if MAC {
        "launchctl bootstrap"
    } else {
        "systemctl --user enable"
    };
    let r = host(Fake::default())
        .once(probe, 1, "")
        .once(probe, 1, "")
        .reply(probe, "")
        .reply(load, "");
    let (_, out) = run(&c, &r, &["setup"]);
    let s = steps(&out);
    assert!(s[0].starts_with("✓ ") && s[0].contains("dev.herdsman.tick"), "{out}");
    assert!(s[1].starts_with("✓ ") && s[1].contains("dev.herdsman.fast"), "{out}");
    assert!(s[2].starts_with("✓ ") && s[2].contains("dev.herdsman.serve"), "{out}");
    assert_eq!(s[3], "= tailscale serve skipped: tailscale is not on PATH");
    assert_eq!(s[4], "✓ profiles synced");
    assert!(
        out.lines()
            .skip(5)
            .any(|l| l.contains(" doctor") || l.contains(" herdr — ")),
        "{out}"
    );
    let serve = std::fs::read_to_string(job(&c, "dev.herdsman.serve")).unwrap();
    assert!(
        serve.contains("127.0.0.1:7777") && serve.contains("serve.log"),
        "{serve}"
    );

    let (_, out) = run(&c, &r, &["setup"]);
    assert!(steps(&out).iter().all(|l| l.starts_with("= ")), "{out}");
    assert!(!r.called("tailscale serve --bg"));
}

#[test]
fn setup_adopts_a_serve_job_under_another_label() {
    let c = ctx(&tmpdir("timer-adopt"), CONFIG);
    let theirs = c.home.join("Library/LaunchAgents/com.someone.herdsman-serve.plist");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(
        &theirs,
        "<plist><array><string>/x/herdsman</string><string>serve</string></array></plist>",
    )
    .unwrap();
    let (_, out) = run(
        &c,
        &host(Fake::default()).reply("launchctl print", ""),
        &["setup", "--no-load"],
    );
    if MAC {
        assert_eq!(
            steps(&out)[2],
            format!("= adopted {} (re-labelling is manual: docs/setup.md)", theirs.display())
        );
        assert!(!job(&c, "dev.herdsman.serve").exists());
    } else {
        assert!(steps(&out)[2].contains("dev.herdsman.serve"), "{out}");
    }
    assert_eq!(
        steps(&out)[3],
        "= tailscale serve skipped: [serve] tailscale_https_port is unset"
    );
}

fn rows(c: &herdsman::Ctx, r: &Fake) -> Vec<(String, &'static str, String, String)> {
    herdsman::timer::services(c, &herdsman::config::Config::load(&c.config).unwrap(), r, None)
}

#[test]
fn services_rows_go_green_amber_and_red() {
    let c = ctx(&tmpdir("timer-svc"), CONFIG);
    let stamp = |w: &str, ago: u64| std::fs::write(c.state.with_file_name(w), (c.now - ago).to_string()).unwrap();
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    stamp("check.at", 60);
    stamp("drive-sendmeter.last", 60);
    // #246: the fast step stamps its start and length.
    std::fs::write(c.state.with_file_name("drive-fast.at"), format!("{} 2.1", c.now - 40)).unwrap();
    let fleet_disk = |gb: u64| {
        std::fs::write(
            c.state.with_file_name("worktrees.du"),
            format!("{} {}", gb << 30, c.now - 240),
        )
        .unwrap()
    };
    fleet_disk(3);
    let df = |gb: u64| {
        format!(
            "Filesystem 1K-blocks Used Available\n/dev/disk1 1 1 {}\n",
            gb * 1_048_576
        )
    };
    let green = host(Fake::default())
        .reply("lsof ", "herdsman 1 u LISTEN\n")
        .reply("df -k /", &df(200));
    let all = rows(&c, &green);
    let names: Vec<_> = all.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "board",
            "tick",
            "drive sendmeter",
            "fast step",
            "phone alerts",
            "herdr",
            "disk",
            "fleet disk"
        ]
    );
    for (item, tone, detail, _) in &all {
        let want = if item == "phone alerts" { "a" } else { "g" }; // no webpush notifier
        assert_eq!(*tone, want, "{item}: {detail}");
    }
    assert_eq!(all[1].2, "ran 60s ago");
    assert_eq!(all[3].2, "fast step 2.1 s · ran 40s ago");
    assert_eq!(all[6].2, "200 GB free");
    assert_eq!(
        all[7].2, "▕█░░░░░░░░░▏ 3.0 / 25 GB · 4m ago",
        "#164: from the last walk, against the budget"
    );

    stamp("check.at", 700);
    let amber = host(Fake::default()).reply("lsof ", "x").reply("df -k /", &df(12));
    let all = rows(&c, &amber);
    fleet_disk(21);
    assert_eq!((all[1].1, all[6].1), ("a", "a"), "{all:?}");
    assert_eq!(rows(&c, &amber)[7].1, "a", "over 80 % of the budget");

    append(
        &c.state,
        &Event::new(
            c.now - 30,
            "herdsman",
            None,
            "tick-failed",
            json!({"key": "tick", "reason": "herdr: gone"}),
        ),
    )
    .unwrap();
    stamp("drive-sendmeter.last", 1000);
    stamp("drive-fast.at", 1000);
    fleet_disk(26);
    let red = Fake::default().reply("df -k /", &df(5));
    let all = rows(&c, &red);
    let tones: Vec<_> = all.iter().map(|r| r.1).collect();
    assert_eq!(tones, ["r", "r", "r", "r", "a", "r", "r", "r"], "{all:?}");
    assert_eq!(all[3].2, "ran 16m ago", "an old stamp without its length");
    assert_eq!(all[7].2, "▕██████████▏ 26.0 / 25 GB · 4m ago");
    assert_eq!(all[1].2, "failed: herdr: gone");
    assert_eq!(
        (all[0].3.as_str(), all[6].3.as_str()),
        ("herdsman setup", "herdsman clean --yes")
    );

    // The page: a SERVICES group with what clears a problem under it.
    let s = herdsman::serve::Server::new(&c, &red, Some("127.0.0.1:7777".into()));
    let page =
        String::from_utf8_lossy(&s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", c.now)).into_owned();
    assert!(page.contains("<span class=gn>SERVICES</span>"), "{page}");
    assert!(
        page.contains("/ 25 GB</span> (104%) <span class=\"mt fx\">· 4m ago</span>"),
        "{page}"
    );
    assert!(
        page.contains("<span class=k>tick</span><span class=w><b class=r>failed: herdr: gone</b> <span class=\"mt fx\">→ herdsman timer install</span>"),
        "{page}"
    );
}

/// #160: a 10 s `du` never reaches a request: the page and the config fragment answer at once; `check` stores the total.
#[test]
fn slow_du_never_blocks_the_board() {
    struct SlowDu(Fake);
    impl herdsman::runner::Runner for SlowDu {
        fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
            if prog == "du" {
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
            self.0.run(prog, args)
        }
    }
    let c = ctx(&tmpdir("timer-slow-du"), CONFIG);
    let r = SlowDu(host(Fake::default()).reply("df -k /", "F\n/dev/disk1 1 1 209715200\n"));
    let s = herdsman::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    for path in ["/", "/frag?id=config&fleet=sendmeter"] {
        let t = std::time::Instant::now();
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n"), c.now);
        assert!(t.elapsed().as_millis() < 500, "{path} took {:?}", t.elapsed());
        assert!(String::from_utf8_lossy(&res).starts_with("HTTP/1.1 200"), "{path}");
    }
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    let du = std::fs::read_to_string(c.state.with_file_name("worktrees.du")).unwrap();
    assert!(du.ends_with(&format!(" {}", c.now)), "{du}");
}
