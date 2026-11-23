#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --quiet --locked --bin drover
exec "${CARGO_TARGET_DIR:-target}/debug/drover" "$@"
