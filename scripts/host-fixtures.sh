#!/usr/bin/env bash
# #141 gate 2: the Host tab's two fixtures, exported beside the demo as <site>/busy and <site>/healthy for
# scripts/ui-audit.sh. Each is a copy of demo/ with a webpush device and 42 GB free; busy: load/cpu 4.24 over 2.5,
# herdr half updated, docs' drive refused SERIAL_BUSY; healthy: load/cpu 1.84, docs' drive started #10.
# Usage: scripts/host-fixtures.sh [site-dir] [herdsman-binary]
set -euo pipefail
site=${1:-site} bin=${2:-./target/release/herdsman}
for fx in busy healthy; do
  d=$(mktemp -d)
  cp -R demo/. "$d"
  python3 - "$d" "$fx" <<'EOF'
import json, sys
d, fx = sys.argv[1:]
busy = fx == "busy"
r = json.load(open(f"{d}/runner.json"))
r["sysctl -n vm.loadavg"] = "{ 33.92 30.10 28.40 }\n" if busy else "{ 14.72 13.10 12.40 }\n"
r["df -k /"] = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 400000000 44040192 91% /\n"
if busy:  # the server is the second `version:` line
    head, tail = r["herdr status"].rsplit("version: 0.9.5", 1)
    r["herdr status"] = head + "version: 0.9.0" + tail
json.dump(r, open(f"{d}/runner.json", "w"))
ts = max(json.loads(l)["ts"] for l in open(f"{d}/events.jsonl") if l.strip())
if busy:
    why = "serial #6 is unmerged (impl), #7 is unmerged (fix(1)), #8 is unmerged (review(1)); retry after they merge"
    d_ = {"step": "start", "cmd": "herdsman spawn docs 10 impl", "result": f"REFUSED SERIAL_BUSY: {why}", "code": "SERIAL_BUSY"}
else:
    d_ = {"step": "start", "cmd": "herdsman spawn docs 10 impl", "result": '"docs-impl-10" ["claude" "fleet-impl"]', "code": None}
with open(f"{d}/events.jsonl", "a") as f:
    f.write(json.dumps({"ts": ts - 420, "fleet": "docs", "issue": 10, "type": "driven", **d_}) + "\n")
toml = open(f"{d}/fleet.toml").read().replace("[fleet.docs]\n", '[fleet.docs]\nserial_label = "database"\n')
open(f"{d}/fleet.toml", "w").write(toml + '\n[[notify]]\ntype = "webpush"\nsub = "mailto:demo@example.com"\n')
import os
os.makedirs(f"{d}/webpush", exist_ok=True)
sub = {"endpoint": "https://web.push.apple.com/demo", "p256dh": "p", "auth": "a", "label": "iPhone Safari", "created": ts - 28800}
json.dump([sub], open(f"{d}/webpush/subs.json", "w"))
EOF
  HERDSMAN_HOME="$d" FLEET_CONFIG="$d/fleet.toml" FLEET_STATE="$d/events.jsonl" "$bin" serve --export "$site/$fx" --demo
  rm -rf "${d:?}"
done
