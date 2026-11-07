#!/bin/sh
S="$HERDSMAN_SCRATCH_TARGET"; mkdir -p "$S/target"
python3 -c 'import fcntl,sys,time; f=open(sys.argv[1],"w"); fcntl.flock(f,fcntl.LOCK_EX); time.sleep(7200)' "$S/.cargo-lock" &
LOCK=$!
CARGO_TARGET_DIR="$S/target" cargo clippy -q --all-targets -- -D warnings > clippy.log 2>&1; echo "EXIT $?" >> clippy.log
CARGO_TARGET_DIR="$S/target" cargo run -q -p herdsman -- cap-check --config ci/cap.toml > cap.log 2>&1; echo "EXIT $?" >> cap.log
kill "$LOCK"
