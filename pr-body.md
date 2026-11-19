Closes #1272

## For humans
A host can now be marked as having its GitHub token issued outside drover (boat VMs are). Such a host no longer warns that the token is about to expire or tells you to make a new one and re-bootstrap.
Every other cloud host keeps the expiry warning as before, and `doctor` now shows it too.

## What changed
- New per-host setting `[hosts.<name>] github_token = "managed" | "external"` (default `"managed"`), per the owner's decision q1952.
- `HostCfg::watches_token()`: a cloud host whose `github_token` is `managed`. `hosts refresh` (survey) and `doctor` both use it, so both run the token look under the same rule. Non-cloud hosts still get no look, as on staging.
- `doctor` adds the token row for those hosts. It showed none before, and the AC asks for it there.
- Test `an_external_github_token_never_warns_of_its_expiry` (crates/cli/tests/host_sync.rs): a `cloud = "boat"` host whose token expires in 0d. With `github_token = "external"` there is no `token expires` line in `hosts refresh` or `doctor`; with the default, both show the line. The test fails when `watches_token` ignores the setting (exit 101).
- docs/hosts.md documents the setting.
- Fix round: I replaced the hardcoded `cloud = "boat"` check with the setting, and dropped the earlier extension of the look to non-cloud hosts (the reviewer's non-blocking note). I also put back the hosts.rs / host_sync.rs fakes that change had needed.

src/ net +17 (19 added, 2 removed) (no budget given): the enum, the field, a predicate and two call sites.

## Evidence at head bf3370fc8f59b3370f9ee37247e42883073c2d30
Update round: merged origin/staging. The only conflict was crates/cli/tests/host_sync.rs: staging removed `a_skipped_sync_logs_why_once_per_streak` and its gh fake; I kept staging's side and added the new `github_token` test.
- `cargo test -p drover --test hosts --test host_sync`: exit 0 (47 + 5 passed, `an_external_github_token_never_warns_of_its_expiry` ok)
- `cargo fmt --check`: exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings`: exit 0
- `cargo test --workspace --no-fail-fast`: exit 101 on one test, `shed::a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it`. It depends on the host (a real `shed` is on this host's PATH), it failed on base in the earlier round too, and this change doesn't touch it. 226 other test binaries passed.
