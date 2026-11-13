//! #9: set-but-empty path variables resolve like unset ones. One test: it mutates the
//! process env, which only this test binary sees.
use drover::Ctx;
use std::path::Path;

#[test]
fn empty_values_are_unset() {
    let h = std::env::temp_dir().join(format!("fleet-test-empty-env-{}", std::process::id()));
    std::env::set_var("HOME", &h);
    for k in [
        "FLEET_CONFIG",
        "FLEET_STATE",
        "FLEET_WORKTREES",
        "FLEET_HERMES_PROFILES",
        "DROVER_HOME",
    ] {
        std::env::set_var(k, "");
    }
    let c = Ctx::from_env();
    assert_eq!(c.config, h.join(".config/drover/fleet.toml"));
    assert_eq!(c.state, h.join(".local/state/drover/events.jsonl"));
    assert_eq!(c.worktrees, h.join(".local/share/drover/work")); // #630
    assert_eq!(c.homes["hermes"], h.join(".hermes/profiles"));
    assert_eq!(c.home, h);
    assert!(c.hints.is_empty() && !Path::new(&h).exists(), "{:?}", c.hints);
}
