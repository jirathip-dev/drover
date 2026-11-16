Closes #1167

## For humans
Drive no longer says every host is full when one host is offline or when starts are still in flight for some other host.
An offline host is never named in the "waiting for a slot" line. A pending start only takes room on the host it was placed on.

## What changed
- `crates/core/src/hosts.rs` `slot_before`: skips hosts whose last look is `up: false`. It binds each pending start to the `host` of its `spawn-started` event and counts it only there. Unbound starts fill the remaining room of the fitting up hosts in order, so a one-host setup still holds kicked starts (the existing `kicked_starts_count_against_the_cap_before_any_lane_is_up` test).
- `crates/cli/tests/host_slots.rs`: three tests, one per AC (commit 1, repro).

src/ net +23 (budget: none stated). The extra lines are the per-host binding and the spread of unbound starts.

## Repro
Commit 1 (4b36e63a0) on base: `cargo test -p drover --test host_slots` exit 101, `left: Some("waiting for a slot: 4/4 working on a")`.

## Evidence at head 0abebd3de6ed423e4e876852912e4900e70c3db8
- `cargo fmt --check` exit 0
- `cargo clippy --workspace --lib --bins --locked -- -D warnings` exit 0
- `cargo clippy --workspace --tests --locked -- -D warnings -A …` (CI's flags) exit 0
- `cargo test -p drover --test host_slots --test drive_kicks --test drive --test drive_sim --test host_offline --test arch_check` exit 0
