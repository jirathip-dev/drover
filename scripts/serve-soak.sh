#!/usr/bin/env bash
# #895: serve's RSS under load: `scripts/serve-soak.sh <secs> <state.db> [dir]` copies the database into dir, runs
# `drover serve` on it with 8 /live clients, the page and herd.json polled every second and a drive pass every 60 s,
# and prints `rss_kb <secs> <kb>` at the start, every 60 s and at the end. DROVER names the binary.
set -euo pipefail
secs=${1:?usage: serve-soak.sh <secs> <state.db> [dir]}
db=${2:?usage: serve-soak.sh <secs> <state.db> [dir]}
dir=${3:-$(mktemp -d)}
bin=${DROVER:-${CARGO_TARGET_DIR:-target}/release/drover}
mkdir -p "$dir/home" "$dir/work"
rm -f "$dir/state.db" "$dir/state.db-wal" "$dir/state.db-shm"
sqlite3 "$db" ".backup '$dir/state.db'"
printf '[serve]\nhooks_addr = "127.0.0.1:0"\n' > "$dir/fleet.toml"
export FLEET_CONFIG="$dir/fleet.toml" FLEET_STATE="$dir/events.jsonl" FLEET_WORKTREES="$dir/work" DROVER_HOME="$dir/home"
export PATH=/usr/bin:/bin
unset DROVER_LANE DROVER_SERVE_FDS
pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT
"$bin" serve --addr 127.0.0.1:0 2> "$dir/serve.log" &
serve=$!
pids+=("$serve")
until port=$(grep -oE 'phone page on http://127.0.0.1:[0-9]+' "$dir/serve.log" | grep -oE '[0-9]+$'); do
  kill -0 "$serve" && sleep 0.2
done
rss() { echo "rss_kb $1 $(ps -o rss= -p "$serve" | tr -d ' ')"; }
curl -s -o /dev/null "http://127.0.0.1:$port/"
rss 0
for _ in 1 2 3 4 5 6 7 8; do
  curl -sN -o /dev/null "http://127.0.0.1:$port/live" &
  pids+=("$!")
done
t0=$SECONDS next=60
while ((SECONDS - t0 < secs)); do
  curl -s -o /dev/null "http://127.0.0.1:$port/" || true
  curl -s -o /dev/null "http://127.0.0.1:$port/herd.json" || true
  if ((SECONDS - t0 >= next)); then
    "$bin" drive > /dev/null 2>&1 || true
    rss "$next"
    next=$((next + 60))
  fi
  sleep 1
done
rss "$secs"
