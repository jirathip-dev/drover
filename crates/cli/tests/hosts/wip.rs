use super::*;
use drover::runner::{Out, Runner};

struct RealGit<'a>(OverSsh<'a>, String);

impl Runner for RealGit<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let line = args
            .last()
            .filter(|l| l.starts_with(&format!("{GIT} '-C' '{}' ", self.1)));
        match line {
            Some(l) if prog == "ssh" => {
                self.0.f.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
                drover::runner::Real.run("sh", &drover::runner::argv(&["-c", l]))
            }
            _ => self.0.run(prog, args),
        }
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if input.is_empty() {
            return self.run(prog, args);
        }
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn git(a: &[&str]) -> String {
    let o = std::process::Command::new("git").args(a).output().unwrap();
    assert!(o.status.success(), "{a:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

const OUTBOX: &str = r#"{"args":["ask","sendmeter","12","--why","bazzite is slow"],"lane":"sendmeter-impl-12","ts":1}"#;

fn superseded(name: &str, src: Option<(&str, &str)>) -> (i32, String, String, FakeShed) {
    let c = setup(name, "", true, 0);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let public = cfg.replacen(
        "integration = \"staging\"",
        "integration = \"staging\"\npublic = true",
        1,
    );
    std::fs::write(&c.config, public).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let d = c.state.parent().unwrap().parent().unwrap().to_path_buf();
    let (origin, wt) = (d.join("origin.git"), d.join("wt"));
    let (origin, wt) = (origin.to_string_lossy(), wt.to_string_lossy().into_owned());
    git(&["init", "-q", "--bare", &origin]);
    git(&["clone", "-q", &origin, &wt]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        git(&["-C", &wt, "config", k, v]);
    }
    std::fs::create_dir_all(format!("{wt}/.drover")).unwrap();
    std::fs::create_dir_all(format!("{wt}/src")).unwrap();
    std::fs::write(format!("{wt}/.drover/rules.md"), "- don't\n").unwrap();
    std::fs::write(format!("{wt}/src/a.rs"), "fn a() {}\n").unwrap();
    git(&["-C", &wt, "add", "-A"]);
    git(&["-C", &wt, "commit", "-qm", "init"]);
    git(&["-C", &wt, "push", "-q", "origin", "HEAD"]);
    std::fs::write(format!("{wt}/.drover/outbox.jsonl"), format!("{OUTBOX}\n")).unwrap();
    if let Some((p, s)) = src {
        std::fs::write(format!("{wt}/{p}"), s).unwrap();
    }
    let sp = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                    "worktree": wt, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(&c, NOW - 900, 12, "spawn", sp);
    let f = live_fake()
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "")
        .reply(&format!("{SSH}{GIT} '-C'"), "")
        .reply("date -r", "20261120-120000\n");
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    bazzite.lane(IMPL, "idle", &wt);
    let r = RealGit(two(&f, &bazzite), wt);
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    let wip = git(&[
        "--git-dir",
        &origin,
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads/wip/",
    ]);
    let wip = wip.trim();
    let files = if wip.is_empty() {
        String::new()
    } else {
        git(&["--git-dir", &origin, "diff", "--name-only", "HEAD", wip])
    };
    (code, out, files, bazzite)
}

#[test]
fn a_supersede_takes_no_snapshot_of_drovers_own_outbox() {
    let (code, out, files, bazzite) = superseded("hosts-wip-outbox", None);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("PUBLIC_LEAK"), "{out}");
    assert_eq!(files, "", "no wip snapshot of the outbox: {out}");
    assert!(bazzite.called("run", IMPL), "{:?}", bazzite.methods());
}

#[test]
fn a_supersede_snapshot_holds_the_source_file_and_not_the_outbox() {
    let (code, out, files, bazzite) = superseded("hosts-wip-src", Some(("src/a.rs", "fn a() { 1; }\n")));
    assert_eq!(code, 0, "{out}");
    assert_eq!(files, "src/a.rs\n", "{out}");
    assert!(bazzite.called("run", IMPL), "{:?}", bazzite.methods());
}

#[test]
fn a_host_name_in_a_product_file_is_still_refused_public_leak() {
    let (code, out, files, bazzite) = superseded(
        "hosts-wip-leak",
        Some(("src/a.rs", "fn a() {}\nconst H: &str = \"ssh bazzite\";\n")),
    );
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PUBLIC_LEAK") && out.contains("src/a.rs:2 has a host"),
        "{out}"
    );
    assert_eq!(files, "", "nothing pushed");
    assert!(!bazzite.called("run", IMPL) && bazzite.has(IMPL));
}

#[test]
fn an_impl_lanes_untracked_file_with_a_home_path_is_still_refused_public_leak() {
    let (code, out, files, bazzite) = superseded("hosts-wip-home", Some(("e2e-dir.txt", "/home/me/e2e\n")));
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PUBLIC_LEAK") && out.contains("e2e-dir.txt:1 has a home-path"),
        "{out}"
    );
    assert_eq!(files, "", "nothing pushed");
    assert!(!bazzite.called("run", IMPL) && bazzite.has(IMPL));
}
