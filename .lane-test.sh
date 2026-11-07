#!/bin/sh
# Lane tests in the scratch target, holding a .cargo-lock flock there so the scratch sweep skips it. Args: cargo test args.
S="$HERDSMAN_SCRATCH_TARGET"
mkdir -p "$S/target"
python3 -c 'import fcntl,sys,time; f=open(sys.argv[1],"w"); fcntl.flock(f,fcntl.LOCK_EX); time.sleep(7200)' "$S/.cargo-lock" &
LOCK=$!
env -u HERDSMAN_SCRATCH_TARGET CARGO_TARGET_DIR="$S/target" cargo test -q "$@" > "$LOG" 2>&1
echo "EXIT $?" >> "$LOG"
kill "$LOCK"
