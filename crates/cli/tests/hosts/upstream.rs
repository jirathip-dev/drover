use super::*;

fn sh_ok(line: &str) -> String {
    let o = std::process::Command::new("sh").args(["-c", line]).output().unwrap();
    assert!(o.status.success(), "{line}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn a_remote_worktree_tracks_its_own_issue_branch_so_a_plain_push_works() {
    let c = setup("hosts-upstream", "", true, 0);
    let d = tmpdir("hosts-upstream-git");
    let (origin, local, wt) = (d.join("origin.git"), d.join("local"), d.join("wt"));
    let q = |p: &std::path::Path| p.display().to_string();
    sh_ok(&format!("git init -q --bare -b staging '{}'", q(&origin)));
    sh_ok(&format!(
        "git clone -q '{1}' '{0}' && git -C '{0}' -c user.email=t@t -c user.name=t commit -q --allow-empty -m base \
         && git -C '{0}' push -q origin HEAD:staging",
        q(&local),
        q(&origin)
    ));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let mut r = remote_fake();
    let none = drover::runner::Out {
        code: 1,
        ..drover::runner::Out::default()
    };
    let verify = format!("{SSH}{GIT} '-C' '/tmp/does-not-matter/sendmeter' 'rev-parse' '--verify'");
    r.replies.insert(0, (verify, none));
    let args = ["spawn", "sendmeter", "12", "impl", "--host", "bazzite"];
    let (code, out) = run(&c, &acp(&r, &bazzite), &args);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let sent = format!("{SSH}{GIT} '-C' '/tmp/does-not-matter/sendmeter' ");
    let calls = r.calls.borrow();
    let git: Vec<&String> = calls.iter().filter(|l| l.starts_with(&sent)).collect();
    assert!(git.iter().any(|l| l.contains("'worktree' 'add'")), "{calls:?}");
    for l in git {
        let l = l.trim_start_matches(SSH);
        let l = l
            .replace("/tmp/does-not-matter/sendmeter", &q(&local))
            .replace(WT, &q(&wt));
        std::process::Command::new("sh").args(["-c", &l]).output().unwrap();
    }
    let w = q(&wt);
    sh_ok(&format!(
        "git -C '{w}' -c user.email=t@t -c user.name=t commit -q --allow-empty -m work && git -C '{w}' push -q"
    ));
    assert_eq!(
        sh_ok(&format!("git -C '{w}' rev-parse --abbrev-ref '@{{u}}'")),
        "origin/issue-12"
    );
    assert_eq!(
        sh_ok(&format!("git -C '{}' rev-parse issue-12", q(&origin))),
        sh_ok(&format!("git -C '{w}' rev-parse HEAD"))
    );
}

#[test]
fn a_respawn_leaves_its_remote_host_under_min_free_gb() {
    let c = setup("hosts-disk-respawn", "", true, 0);
    remote_spawn(&c, NOW - 3600);
    let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda 9 8 4194304 90% /\n";
    let low = fake(70, 97).reply(&format!("{SSH}'df' '-k' '/'"), df);
    assert_eq!(placed(&c, &low, 12, "impl", None).as_deref(), Ok("mini"));
    let cache = c.state.parent().unwrap().join("hosts.json");
    let text = std::fs::read_to_string(&cache)
        .unwrap()
        .replace("\"disk\":200.0", "\"disk\":4.0");
    std::fs::write(&cache, text).unwrap();
    let (_, listed) = run(&c, &low, &["hosts"]);
    let bazzite = listed.lines().find(|l| l.starts_with("bazzite ")).unwrap_or_default();
    assert!(
        bazzite.contains("up · DISK_LOW 4.0 GB free < 10: no new lanes"),
        "{listed}"
    );
}
