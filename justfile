# #375: the board's TS types, `assets/types/api.d.ts`, from the Rust payload types (crates/server/src/api.rs and the
# core rows); CI fails when the committed file differs.
types:
    cargo test -p drover-server --features ts export_bindings

# Every gate CI runs (the core, rust and ts packs and .gatekeeper/gates/), as CI runs them; `drover turn done` runs only the light checks (#1863).
check:
    ./.gatekeeper/run check

# #1147: every generated file (.drover/arch.toml `generated`); arch-check runs it and fails on a difference.
gen:
    bash scripts/gen.sh

# #680: the deterministic simulation's long batch: `just sim [n]` runs n (default 500) random seeds from DROVER_SIM_FROM
# (default 1000), each failing seed printed with its trace; replay one with `DROVER_SIM_SEED=<n> cargo test -p
# drover --test sim`. CI runs the fixed seed set (crates/cli/tests/sim/seeds.rs) in `cargo test`;
# .github/workflows/sim.yml runs `just sim 2000` and `just sim --hunt 10` nightly.
# #688: `just sim --hunt [minutes]` (default 10) is the coverage-guided search: its corpus and each minimised failure
# (a seeds.rs line, `DROVER_SIM_PLAN=…` replays it) go to target/sim-corpus.
# #685: `just sim --metrics [N] [--json]` prints the fleet metrics over N seeds (default 50) as a table, or JSON;
# DROVER_SIM_BASELINE=<metrics.json> adds a baseline column and a warning per metric over 20% worse.
# #686: `just sim --replay <state.db|scenario.json> [--days 7] [--fleet herdsman] [--save scenario.json]` replays a real
# window of a fleet's log (read-only) in the sim: the invariants' result and the real-vs-sim metrics table.
# #687: `just sim --what-if <fleet.toml copy> [--current <fleet.toml>] [--fleet herdsman] [--replay <state.db|scenario.json>
# [--days 7] | N]` runs the same scenarios (a replayed window, else N seeds, default 50) under the current config
# (default $FLEET_CONFIG, else ~/.config/drover/fleet.toml) and the candidate, read-only, and prints the metric deltas.
sim *args:
    #!/usr/bin/env bash
    set -euo pipefail
    set -- {{args}}
    abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
    if [ "${1:-}" = --what-if ]; then
        export DROVER_SIM_WHATIF DROVER_SIM_CURRENT; DROVER_SIM_WHATIF=$(abs "$2"); shift 2
        DROVER_SIM_CURRENT=$(abs "${FLEET_CONFIG:-$HOME/.config/drover/fleet.toml}")
        while [ $# -gt 0 ]; do
            case $1 in [0-9]*) export DROVER_SIM_N="$1"; shift; continue ;; esac
            test $# -gt 1 || { echo "just sim --what-if: $1 needs a value" >&2; exit 2; }
            case $1 in
                --current) DROVER_SIM_CURRENT=$(abs "$2") ;;
                --replay) export DROVER_SIM_REPLAY; DROVER_SIM_REPLAY=$(abs "$2") ;;
                --days) export DROVER_SIM_DAYS="$2" ;;
                --fleet) export DROVER_SIM_FLEET="$2" ;;
                *) echo "just sim --what-if: unknown $1" >&2; exit 2 ;;
            esac
            shift 2
        done
        exec cargo test --release --locked -p drover --test sim what_if -- --ignored --exact --nocapture
    fi
    if [ "${1:-}" = --replay ]; then
        export DROVER_SIM_REPLAY; DROVER_SIM_REPLAY=$(abs "$2"); shift 2
        while [ $# -gt 1 ]; do
            case $1 in
                --days) export DROVER_SIM_DAYS="$2" ;;
                --fleet) export DROVER_SIM_FLEET="$2" ;;
                --save) export DROVER_SIM_SAVE; DROVER_SIM_SAVE=$(abs "$2") ;;
                *) echo "just sim --replay: unknown $1" >&2; exit 2 ;;
            esac
            shift 2
        done
        test $# -eq 0 || { echo "just sim --replay: $1 needs a value" >&2; exit 2; }
        exec cargo test --release --locked -p drover --test sim replay_db -- --ignored --nocapture
    fi
    if [ "${1:-}" = --hunt ]; then
        DROVER_SIM_HUNT_MINUTES="${2:-10}" DROVER_SIM_CORPUS="${DROVER_SIM_CORPUS:-{{justfile_directory()}}/target/sim-corpus}" \
            exec cargo test --release --locked -p drover --test sim hunt -- --ignored --exact --nocapture
    fi
    if [ "${1:-}" != --metrics ]; then
        DROVER_SIM_N="${1:-500}" exec cargo test --release --locked -p drover --test sim random_seeds -- --ignored
    fi
    n=50 ext=md
    for a in "${@:2}"; do if [ "$a" = --json ]; then ext=json; else n="$a"; fi; done
    out="${DROVER_SIM_OUT:-${CARGO_TARGET_DIR:-target}/sim-metrics}"
    mkdir -p "$out" && out=$(cd "$out" && pwd)
    DROVER_SIM_N="$n" DROVER_SIM_OUT="$out" cargo test --release --locked -p drover --test sim metrics_batch -- --ignored >&2
    cat "$out/metrics.$ext"
