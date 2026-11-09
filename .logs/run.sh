#!/bin/bash
cd "$(dirname "$0")/.."
: > .logs/exits.txt
cargo test --workspace --lib --bins > .logs/lib.log 2>&1; echo "lib $?" >> .logs/exits.txt
for p in crates/server/tests/*.rs; do t=$(basename "$p" .rs); cargo test -p herdsman-server --test "$t" > ".logs/srv-$t.log" 2>&1; echo "srv-$t $?" >> .logs/exits.txt; done
for p in crates/cli/tests/*.rs; do t=$(basename "$p" .rs); cargo test -p herdsman --test "$t" > ".logs/$t.log" 2>&1; echo "$t $?" >> .logs/exits.txt; done
cargo test --workspace --doc > .logs/doc.log 2>&1; echo "doc $?" >> .logs/exits.txt
echo ALLDONE >> .logs/exits.txt
