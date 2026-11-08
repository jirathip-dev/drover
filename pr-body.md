Closes #609

## For humans
Every lane now says where it runs. The board's Queue and Lanes rows, the API, and `status` / `board` / `waiting` show one short label such as "mini · shed", "air" or "shed".
The label only shows what varies: the host appears while two or more hosts are enabled, and the runtime (herdr or shed) appears while live lanes use both. When nothing varies there is no label.
A lane keeps the runtime it was started with, even if the fleet's settings change later.

## What changed
- `Lane` (core `lanes.rs`) carries `runtime` (`herdr` | `shed` | `shed-acp`, from the lane's spawn event; spawn now logs `shed-acp` for ACP sessions) and `place` (the `where` label, Amendment C). Shed lanes get their host too: the main host's name when they have none.
- API v1 `lanes`, `state`, `queue` rows (the active lane) and `/herd.json` add `runtime`, `host` and `where`. These are additive fields. The contract goldens are updated, and `golden/api_v1/where.json` holds all four shapes. Documented in `docs/serve.md` and `docs/hosts.md`; `assets/types/api.d.ts` is regenerated.
- CLI: `status` HOST column, `board` row (`on …`) and `waiting` line show the same `where` string.
- Old board: Queue and Lanes rows show one `where` chip (existing `tag m` style), with no chip when it is empty.
- Conductor lanes go through the same `lanes::join`, so they carry the same fields.

## Tests
- `every_lane_says_where_it_runs` (crates/cli/tests/shed.rs): a herdr lane on remote `air` and a shed-acp lane on main `mini`. It checks `/api/v1/lanes`, `/api/v1/queue`, `state` and `herd.json` for each lane's runtime/host/where in all four shapes, including when a host is disabled and when the last herdr lane ends. It also covers the CLI's status/board/waiting text. (AC2, Amendment C.)
- `a_lane_keeps_its_spawn_runtime_after_the_config_changes` (AC3).

## Evidence (head HEADSHA)
EVIDENCE

## Notes
- AC4 vs Amendment C: on the live host every live lane is herdr. Under Amendment C (only what varies) `status herdsman` therefore shows `mini` in HOST and no runtime word. The runtime appears once a shed lane is live alongside a herdr one.
- Line budget: src/ net +86 (97 added, 11 removed). The new data fields, the label rule and the three CLI surfaces account for it.
- The 390-px screenshot check (a Queue row and a Lanes row each show the `where` chip) is left to the `ui` job and the verify stage; I did not take screenshots in this lane.
