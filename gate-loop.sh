#!/usr/bin/env bash
skip=(--skip the_thread_follows_its_end --skip the_card_lands_live_in_view --skip live_board_chat_and_ascii_in_webkit)
: > gate-loop.txt
run() { local name=$1; shift
  for try in 1 2; do
    env -u HERDSMAN_LANE cargo test "$@" -- "${skip[@]}" > "gate-logs/$name.log" 2>&1; rc=$?
    grep -q "never executed" "gate-logs/$name.log" || break
  done
  echo "$name $rc" >> gate-loop.txt
}
run workspace-others --workspace --exclude herdsman
run herdsman-lib -p herdsman --lib --bins
for f in crates/cli/tests/*.rs; do t=$(basename "$f" .rs); run "$t" -p herdsman --test "$t"; done
echo done >> gate-loop.txt
