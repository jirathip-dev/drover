#!/usr/bin/env bash
# #1147: every generated file (.drover/arch.toml `generated`): `just gen`, arch-check on a lane and CI's local/gen run it.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test -q --locked -p drover-server --features ts export_
npm --prefix web ci --no-audit --no-fund
npm --prefix web run build
cargo run -q --locked -p drover -- diagram > docs/workflow.md
BLESS=1 cargo test -q --locked -p drover --test brief_golden
BLESS=1 cargo test -q --locked -p drover --test flow_table
cargo run -q --locked -p drover -- config show --md > docs/config/limits.md
find fleet.example.d -type f | LC_ALL=C sort | xargs cat > fleet.example.toml
./.gatekeeper/run fixture core/hygiene/shared-names --to crates/cli/tests/golden/private-names
