Closes #1264

Staging build fix: re-pins the sim golden `the_known_fleet_gives_exact_metrics`.

## Cause
#1180 (#951) re-pinned this golden at its own head, on a base without #954 (#1199, 9481b85). The squash merge combined both. #954 makes a fast drive pass skip finished issues (`crates/core/src/drive.rs`, `!(fast && s.done())`), so after #201 merges, fast passes no longer take its per-issue locks. Sim time advances per command, so the timer phase shifts and #202 merges 3 s later. Nothing is decided differently: spawns, wasted, supersedes, lanes, CI wait, lock wait, merges and rounds are all unchanged.

Proof: on this head with only that #954 filter reverted, the old golden passes (`to_merge [927, 1250]`, exit 0). With it back in place, the test gets `[927, 1253]`.

## Numbers that moved
- `to_merge`: [927, 1250] → [927, 1253] (#202 +3 s)
- `idle`: 984 → 987 (the same 3 s)
- `idle_lane_min`: 16.4 → 16.45
- `queue_to_merge_p90_min`: 20.83 → 20.88

## Evidence (head 60719c18c38e514fa41f48d569e0c73cbbd2ce63)
- `cargo test -p drover --test sim` → exit 0 (13 passed, 6 ignored)
- `cargo fmt --check` → exit 0

src/ net +0 (budget n/a): test-only change.

## For humans
The test suite on the shared branch goes green again, so other changes can merge.
Two recent speed-ups each passed alone. Together, they moved one simulated merge by three seconds, and a test still expected the old time.
How drover behaves does not change.
