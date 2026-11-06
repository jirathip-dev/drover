Closes #592

## For humans
Lanes on other machines no longer need a copy of the fleet settings there. When a lane starts, the main machine works out its safety rules and sends them along, so a fleet added later works on every host straight away.
If the settings change while such a lane runs, the board marks it "rules stale" until it is restarted. `herdsman doctor` points out an old settings copy left on another machine (it never deletes it).

## What changed
- `hook::Rules` (protected branches, deny list under the host's rules, conductor groups, reviewer flag, builds flag) is resolved on the main host. For a remote lane, spawn writes it to `<worktree>.rules.json` there, exports `HERDSMAN_RULES`, and logs it on the `spawn` event. It holds no notifier, token or env value.
- The hook (and the dsh pre-push hook) reads only that file when `HERDSMAN_RULES` is set. A missing or unreadable file fails closed: `HOOK_FAILED_CLOSED: rules file <path> is unreadable (<error>)`. The file is a protected path, so the lane can't edit it.
- Main-host lanes keep resolving from the live fleet.toml (decision 1: only the remote hook reads the file), so only remote lanes can be `rules stale`.
- Board: `rules stale: respawn to apply` under a live lane whose frozen rules differ from what fleet.toml resolves now; `waiting`/status append ` · rules stale`. A respawn logs fresh rules, which clears it.
- Doctor: `! stale fleet.toml on <host>: no longer read → rm <path> there` for each reachable remote host that still has one. Nothing is removed.
- docs/hosts.md: new "A remote lane's hook rules" section.

## Open: cloud bootstrap (AC 6), ask q659
Bootstrap still writes `fleet.toml` in this head. Bootstrap and the host self-update both run `herdsman profiles sync` on the host, and that needs a fleet.toml. Without one, a new cloud host gets no harness profiles and its lanes can't start. I'm waiting on the owner's answer (q659) before dropping that write.

## Repro
`cargo test -p herdsman --test remote_rules a_remote_lanes_hook_judges_a_fleet_its_host_never_heard_of`: commit 1 (dd036512) fails on base with `REFUSED HOOK_FAILED_CLOSED: fleet \`sendmeter\` is not in fleet.toml` and passes at the head.

## Evidence at f224a01
- `cargo test --locked -p herdsman --test remote_rules --test hosts --test hook --test dsh --test cloud --test host_offline` → exit 0
- `cargo fmt --check` → exit 0; `cargo clippy --all-targets --locked -- -D warnings` → exit 0 (pre-merge of staging)
- Full `cargo test --locked --no-fail-fast` locally: every binary passed up to `ui_audit`. That one hung and the run was killed (a host issue, it doesn't touch this code). The binaries after it passed separately. CI is the full gate.
- `herdsman cap-check --config ci/cap.toml` → exit 0 (30678 / 31500)

src/ net +74 (budget: none stated). The rules struct and its resolver replace `deny_list`/`protected_branches`; the rest is the spawn write, the stale mark and the doctor row.
