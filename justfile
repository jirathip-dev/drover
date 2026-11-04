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
