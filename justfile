# #375: the board's TS types, `assets/types/api.d.ts`, from the Rust payload types (crates/server/src/api.rs and the
# core rows); CI fails when the committed file differs.
types:
    cargo test -p herdsman-server --features ts export_bindings

# #375: no board TS under assets/ (but assets/types/) declares its own copy of a generated type.
types-lint:
    #!/usr/bin/env bash
    set -euo pipefail
    names=$(grep -oE '^export type [A-Za-z]+' assets/types/api.d.ts | cut -d' ' -f3 | paste -sd'|' -)
    bad=$(grep -rnE --include='*.ts' "\b(type|interface|class) ($names)\b" assets | grep -v '^assets/types/' || true)
    test -z "$bad" || { echo "hand-written copy of a generated type (import it from assets/types/api): $bad"; exit 1; }

# #269: crates/server builds no HTML but the page shell (`page`, its `pairb`, and the export's copy of it): every other
# Rust string with a tag that has attributes or a closing tag fails; the board (web/) draws the rest from API v1.
html-lint:
    #!/usr/bin/env bash
    set -euo pipefail
    tags='p|div|span|b|i|a|button|form|input|details|summary|section|pre|code|h[1-6]|svg|use|small|figure|figcaption|img|ul|li|textarea|select|option|label|br|script|style|link|meta|html|head|body|header|main|nav|template'
    bad=$(find crates/server/src -name '*.rs' | sort | xargs awk -v t="$tags" '
      /^[[:space:]]*(pub(\([a-z]+\))? )?fn [a-z_0-9]+/ { match($0, /fn [a-z_0-9]+/); f = substr($0, RSTART + 3, RLENGTH - 3) }
      $0 ~ ("\"[^\"]*(</(" t ")>|<(" t ") [a-z-]+=)") && f !~ /^(page|pairb|export)$/ { print FILENAME ":" FNR ": " $0 }')
    test -z "$bad" || { echo "HTML built outside the page shell (draw it in web/ from API v1): $bad"; exit 1; }

# #680: the deterministic simulation's long batch: `n` random seeds from HERDSMAN_SIM_FROM (default 1000), each failing
# seed printed with its trace; replay one with `HERDSMAN_SIM_SEED=<n> cargo test -p herdsman --test sim`. CI runs the
# fixed seed set (crates/cli/tests/sim/seeds.rs) in `cargo test`; .github/workflows/sim.yml runs `just sim 2000` nightly.
sim n="500":
    HERDSMAN_SIM_N={{n}} cargo test --release --locked -p herdsman --test sim random_seeds -- --ignored
