#!/usr/bin/env bash
# #1147: every generated file (.drover/arch.toml `generated`): `just gen`, arch-check on a lane and CI's local/gen run it.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test -q --locked -p drover-server --features ts export_bindings
npm --prefix web ci --no-audit --no-fund
npm --prefix web run build
cargo run -q --locked -p drover -- diagram > docs/workflow.md
BLESS=1 cargo test -q --locked -p drover --test brief_golden
