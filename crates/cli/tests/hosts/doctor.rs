use super::*;

#[test]
fn doctor_flags_a_stale_remote_fleet_toml_and_leaves_it() {
    let c = setup("hosts-stale-conf", "", true, 0);
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let r = healthy(fake(0.1, 0.1)).reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n");
    let (_, out) = run(&c, &two(&r, &bazzite), &["doctor"]);
    assert!(
        !out.contains("stale fleet.toml") && out.contains("\n! host bazzite sshd — "),
        "{out}"
    );
    let conf = c.config.display().to_string();
    let r = r.reply(&format!("{SSH}'sh' '-c' 'test -e \"$1\"' 'sh' '{conf}'"), "");
    let (_, out) = run(&c, &two(&r, &bazzite), &["doctor"]);
    assert!(out.contains("\n! stale fleet.toml on bazzite: no longer read"), "{out}");
    assert!(!r.calls.borrow().iter().any(|l| l.contains("'rm'") && l.contains(&conf)));
}

#[test]
fn doctor_flags_each_host_without_just_on_the_done_checks_path() {
    let c = setup("hosts-just", "", true, 0);
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let base = || healthy(fake(0.1, 0.1)).reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n");
    let cfg = Config::load(&c.config).unwrap();
    let main = format!(
        "sh -c export PATH='{}'; command -v just",
        drover::timer::job_path(&c, &cfg, &base())
    );
    let remote = format!("{SSH}'sh' '-c' 'command -v just'");
    let r = base().reply(&main, "/usr/bin/just\n").reply(&remote, "/usr/bin/just\n");
    let (_, out) = run(&c, &two(&r, &bazzite), &["doctor"]);
    assert!(!out.contains("just —"), "{out}");
    assert!(r.called(&main) && r.called(&remote), "{:?}", r.calls.borrow());
    let r = base().reply(&main, "/usr/bin/just\n");
    let (_, out) = run(&c, &two(&r, &bazzite), &["doctor"]);
    assert!(
        out.contains("\n! just — missing on bazzite → install just on bazzite\n"),
        "{out}"
    );
    assert!(!out.contains("missing on mini"), "{out}");
    let r = base().reply(&remote, "/usr/bin/just\n");
    let (_, out) = run(&c, &two(&r, &bazzite), &["doctor"]);
    assert!(out.contains("\n! just — missing on mini → install just"), "{out}");
    assert!(!out.contains("missing on bazzite"), "{out}");
}
