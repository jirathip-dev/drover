#!/usr/bin/env python3
"""#367 gate 2 fixture: three repos (herdsman 48 open, synergy 14, berth 1), a 6-step chain, a branching chain and
40+ queued rows, as a copy of demo/ (its runner.json and host data) with its own fleet.toml, events and title cache.
Nothing here is real: the repos are `acme-example/*`; the herdsman titles echo that repo's open issues.
Usage: python3 docs/design/queue-367/fixture.py <out dir>   (run from the repo root)
"""
import json
import shutil
import sys
from pathlib import Path

out = Path(sys.argv[1])
shutil.rmtree(out, ignore_errors=True)
shutil.copytree("demo", out)
for f in (out / "cache").glob("*.json"):
    f.unlink()
runner = json.loads((out / "runner.json").read_text())
pane_text = runner["herdr pane read w1:p2"]

NOW = 1793800000  # the export's now: the last event (60 s ago) + 60 s
H, M = 3600, 60
events, agents, titles = [], [], {}


def ev(ago, fleet, kind, issue=None, **data):
    e = {"ts": NOW - ago, "fleet": fleet, "type": kind, **data}
    if issue is not None:
        e["issue"] = issue
    events.append(e)


def lane(fleet, issue, role, ago, status="working", suffix=""):
    name = f"{fleet}-{role}-{issue}{suffix}"
    ev(ago, fleet, "spawn", issue, role=role, agent=name, harness="claude", model="opus", branch=f"issue-{issue}")
    pane = f"w9:p{len(agents) + 1}"
    agents.append({"name": name, "agent_status": status, "pane_id": pane, "agent": "claude", "cwd": f"/srv/acme/worktrees/{name}"})
    runner[f"herdr pane read {pane}"] = pane_text
    return name


# --- herdsman --------------------------------------------------------------------------------------------------
HS = "herdsman"
hs_titles = {
    265: "Decouple 2/7: JSON API v1 with TypeScript types generated from Rust, a web/ TypeScript scaffold (esbuild)",
    266: "Decouple 3/7: Events tab rendered by the TypeScript board from the API; delete its Rust HTML",
    267: "Decouple 4/7: Host tab rendered by the TypeScript board from the API; delete its Rust HTML",
    268: "Decouple 5/7: Queue tab rendered by the TypeScript board from the API; delete its Rust HTML",
    269: "Decouple 6/7: Lanes tab and chat sheet rendered by the TypeScript board; delete the remaining Rust HTML",
    270: "Decouple 7/7: re-measure and reset the line cap and add a soft web/ budget reported per PR",
    298: "runtime = \"berth\": berth as an alternative terminal runtime behind the herdr adapter",
    300: "Overlap: predict conflicts from hunks, not whole files",
    331: "Drive: one pass per fleet in parallel, a shared host budget",
    340: "Kicks die when the launchd tick job exits, stranding spawn markers and unrecorded lanes",
    344: "Phase B: state in SQLite (WAL): events, leases instead of lock/marker files, devices, snapshot",
    345: "Phase C: one supervised long-running async service (board + drive + watchers, event-driven)",
    356: "Phase B: migrate the events checkpoint into SQLite, keep the jsonl export",
    358: "Phase C: watchers push board deltas instead of the 5 s poll",
    350: "Fast-first landing: PASS + fast checks land at once, full suite runs on staging after",
    351: "Board streams changes, not pages: /live JSON deltas, bounded chat/Events history",
    357: "Line cap 19,000 now, with the per-phase cap plan recorded in cap-history",
    359: "Lane start race: confirm a new lane by its process + the harness startup title over a window",
    360: "Reviewer brief: a test anti-patterns list so review FAILs tests that can't fail",
    365: "Backlog in the Queue tab: every open unqueued issue, search/filters, multi-select",
}
filler = [
    "Host sampling only while someone watches the Host tab; bounded host history",
    "Handover summary when a lane switches agent/account: capped items, delta since last run",
    "Worktree disk reclaim: preview-then-apply, clean build caches only in idle worktrees",
    "Incremental transcript reads: remember each transcript's offset, parse only the tail",
    "Board report mode: tap any element to send its markup and a cropped screenshot",
    "Events tab: a day divider and sticky day headers",
    "Lanes sheet: copy the lane's last command with one tap",
    "Drive: explain a skipped spawn in one line on the fleet card",
    "Config sheet: diff preview before a role switch is saved",
    "Host cards: swap trend as a third sparkline",
    "Verdict files: link each finding to its line on the PR",
    "Notify: quiet hours per device",
    "Queue: drag to reorder on wide screens",
    "Merge: say which gate blocked in the needs-you card",
    "Check: alert once when CI is red on staging for 30 min",
    "Spawn: name the host's free memory in the HOST_BUSY refusal",
    "Pair: show the paired devices with their last use",
    "Ask: a routine ask's countdown on the Lanes row",
    "Cloud hosts: destroy idle hosts after 2 h with a wip save",
    "Docs: one page per command with an example run",
    "Lanes tab: group lanes by host when more than one host runs lanes",
    "Brief: carry the parent issue's decisions into a follow-up's brief",
    "Events: filter by issue from the Queue card",
    "Chat sheet: search inside a lane's transcript",
    "Status: --watch redraws only the changed rows",
    "Serial groups: show the waiting issues on the lock tag",
    "Kill: confirm with the unpushed commit count",
    "Profiles sync: report drift without writing",
    "Board: offline banner names the last good read",
    "Theme: a high-contrast scheme for daylight",
    "Doctor: check the push subscription is still valid",
    "Wait: print the failing check's name and URL",
    "Queue tab: long-press a row to copy its issue link",
    "Review rounds: show the round budget on the card",
    "Hosts: tag probe timeout per host",
    "Merge: retry the evidence comment on a 502",
]
# needs you: an open ask (waiting: decision) and a round limit (needs-human)
hs_queue = [331, 300]
# in progress (queue order): impl, ready, fix, impl, review, impl, impl
hs_queue += [359, 360, 357, 340, 350, 298, 351]
# the 6-step chain (#265 ready, then 266..270 queued, each on the one before) and the branching chain #344
hs_queue += [265, 266, 267, 268, 269, 270, 344, 345, 356, 358, 365]
plain = list(range(371, 371 + len(filler)))
hs_queue += plain
for n, t in zip(plain, filler):
    hs_titles[n] = t

lane(HS, 331, "impl", 2 * H + 31 * M)
ev(18 * M, HS, "needs-decision", 331, id="h7", question="Run one drive pass per fleet in parallel, or keep them in turn with a shared budget?",
   options=["in parallel, a shared host budget", "in turn, as today"], recommend=1, agent="herdsman-impl-331")
lane(HS, 300, "impl", 9 * H)
lane(HS, 300, "rev", 7 * H, status="done", suffix="-r1")
ev(6 * H, HS, "verdict", 300, verdict="FAIL", sha="30" * 20)
ev(59 * M, HS, "needs-human", 300, code="ROUND_LIMIT", reason="review rounds ran out (4/3)")
lane(HS, 359, "impl", 9 * M)
lane(HS, 360, "impl", 3 * H)
lane(HS, 360, "rev", 40 * M, status="done", suffix="-r1")
ev(87, HS, "verdict", 360, verdict="PASS", sha="36" * 20)
lane(HS, 357, "impl", 2 * H)
lane(HS, 357, "rev", 50 * M, status="done", suffix="-r1")
ev(30 * M, HS, "verdict", 357, verdict="FAIL", sha="35" * 20)
lane(HS, 357, "impl", 10 * M, suffix="-f1")
lane(HS, 340, "impl", 1 * H + 47 * M)
lane(HS, 350, "impl", 5 * H)
lane(HS, 350, "rev", 4 * M, suffix="-r1")
lane(HS, 298, "impl", 10 * H + 5 * M, status="idle")
lane(HS, 351, "impl", 9 * M)
lane(HS, 265, "impl", 6 * H)
lane(HS, 265, "rev", 70 * M, status="done", suffix="-r1")
ev(35 * M, HS, "verdict", 265, verdict="PASS", sha="26" * 20)
ev(12 * H, HS, "queue", issues=hs_queue, op="add")
for a, b in [(266, 265), (267, 266), (268, 267), (269, 268), (270, 269), (345, 344), (358, 345), (356, 344), (365, 351)]:
    ev(12 * H, HS, "queue", issues=[a], on=b, op="block")
titles[HS] = hs_titles

# --- synergy ---------------------------------------------------------------------------------------------------
SY = "synergy"
sy_titles = {
    88: "Shared cart: merge two carts when a guest signs in",
    91: "Checkout: Apple Pay on the web",
    93: "Search: typo tolerance for product names",
    94: "Orders: export a month as CSV",
    96: "Inventory: low-stock alert per warehouse",
    97: "Push: order shipped notification",
    98: "Push: delivery window changed",
    99: "Push: quiet hours",
}
sy_queue = [88, 90, 91, 93, 94, 96, 97, 98, 99] + list(range(101, 106))
for n, t in zip(range(101, 106), ["Reviews: photos in a review", "Wishlist: share a list by link", "Coupons: one per account",
                                  "Admin: bulk price edit", "Analytics: funnel by traffic source"]):
    sy_titles[n] = t
sy_titles[90] = "Accounts: delete my account and its data"
lane(SY, 88, "impl", 50 * M)
ev(6 * M, SY, "needs-decision", 88, id="s3", question="Keep the guest cart's items when the signed-in cart already has the same product?",
   options=["keep both quantities", "keep the signed-in cart's"], recommend=1, agent="synergy-impl-88")
lane(SY, 90, "impl", 8 * H)
ev(2 * H, SY, "needs-human", 90, code="ROUND_LIMIT", reason="review rounds ran out (3/3)")
lane(SY, 91, "impl", 25 * M)
lane(SY, 93, "impl", 3 * H)
lane(SY, 93, "rev", 12 * M, suffix="-r1")
lane(SY, 94, "impl", 2 * H + 3 * M)
ev(13 * H, SY, "queue", issues=sy_queue, op="add")
for a, b in [(98, 97), (99, 98)]:
    ev(13 * H, SY, "queue", issues=[a], on=b, op="block")
titles[SY] = sy_titles

# --- berth -----------------------------------------------------------------------------------------------------
BE = "berth"
titles[BE] = {18: "M3: attach to a running session over the socket"}
lane(BE, 18, "impl", 40 * M)
lane(BE, 18, "rev", 102, suffix="-r1")
ev(14 * H, BE, "queue", issues=[18], op="add")

# merged today, so `done today` has rows
for f, n, ago in [(HS, 256, 22 * M), (HS, 255, 4 * H), (SY, 87, 3 * H)]:
    ev(ago + 600, f, "spawn", n, role="impl", agent=f"{f}-impl-{n}", harness="claude", branch=f"issue-{n}")
    ev(ago, f, "merged", n, pr=str(n + 100), into="staging", merge_sha=f"{n:02d}" * 20)
    titles[f][n] = "merged today"

# the last event sets the export's clock (now = it + 60 s)
ev(60, BE, "drive-pass", by="auto", started=NOW - 66, ended=NOW - 60, at="09:00", actions=[])
events.sort(key=lambda e: e["ts"])
(out / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))
for f, ts in titles.items():
    for n, t in ts.items():
        (out / "cache" / f"issue-{f}-{n}.json").write_text(json.dumps({"title": t, "body": "", "updatedAt": "2026-11-01T00:00:00Z"}))
runner["herdr agent list"] = json.dumps({"result": {"agents": agents}})
(out / "runner.json").write_text(json.dumps(runner))
fleet = ['# #367 fixture (docs/design/queue-367/fixture.py). Nothing here is real.', 'models = ["opus", "sonnet"]', "",
         "[limits]", "stall_minutes = 240"]
for f in [HS, SY, BE]:
    fleet += ["", f"[fleet.{f}]", f'repo = "acme-example/{f}"', f'local = "/srv/acme/{f}"', 'integration = "staging"',
              "", f"[fleet.{f}.role.impl]", 'harness = "claude"', 'model = "opus"']
(out / "fleet.toml").write_text("\n".join(fleet) + "\n")
print(out, len(events), "events", sum(len(t) for t in titles.values()), "titles")
