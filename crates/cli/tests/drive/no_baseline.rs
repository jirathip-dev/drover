use super::*;

#[test]
fn a_no_baseline_pass_tells_the_reviewer_once_then_is_one_owner_card() {
    let (mut c, _, r) = proofless("drive-nobaseline", "PASS\n\nAll ACs hold.\n", true);
    let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--perf"]);
    assert_eq!(code, 0, "{out}");
    let told = || {
        typed(
            "sendmeter-rev-12-r1",
            "drover verdict refused your verdict.md: REFUSED NO_BASELINE",
        )
    };
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    assert!(out.contains("REFUSED NO_BASELINE"), "{out}");
    assert_eq!(told(), 1, "{:?}", r.calls.borrow());
    c.now += 600;
    let (out, _) = drive(&c, &r);
    assert_eq!((told(), kinds(&c, "verdict").len()), (1, 0), "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(out.contains("verdict refused NO_BASELINE"), "{out}");
    assert!(!out.contains("nothing drives this"), "{out}");
    c.now += 31 * 60;
    let (out, _) = drive(&c, &r);
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1, "{out}");
    assert_eq!(nh[0].str("code"), Some("NO_BASELINE"), "{out}");
    assert!(nh[0].str("reason").unwrap().contains("NO_BASELINE"), "{nh:?}");
    assert_eq!(state(&c), "needs-human", "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(!out.contains("nothing drives this"), "{out}");
}
