#!/bin/sh
# #1587: arch-check's generator. drover gives it 60 s, which a cold build in CI's core leg (no Rust cache) cannot
# meet, so there it writes nothing and CI's local/gen gate runs scripts/gen.sh; everywhere else it runs scripts/gen.sh.
set -eu
cd "$(dirname "$0")/.."
if [ -n "${CI:-}" ] && [ "${LEG:-}" = core ]; then exit 0; fi
exec bash scripts/gen.sh
