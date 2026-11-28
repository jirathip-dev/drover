# Spawn

`drover spawn` refuses before creating anything:

- `HOST_BUSY`: memory pressure warn or worse, load per CPU over `[limits]`, or a sustained
  swap-out rate (the median of the last 5 samples) over `[limits]` while pressure is warn or worse;
  one spike admits. `--force` overrides and logs an `override` event with `by` (`owner` or `orchestrator-<id>`);
  only the owner and orchestrators pass it, the lane hook refuses it in any other lane (#908).
- `MEM_LOW`: free + inactive memory under `[limits] min_free_mem_gb` (default 2 GB); a lane needs about 0.5–1.5 GB.
- `DISK_LOW`: free disk on `/` under `[limits] min_free_gb` (default 10 GB). `drover clean --yes`
  frees the build folders (`target/`, `node_modules/`, `.next/`, `dist/`) of merged or closed lanes,
  even kept ones (never a live lane's, never one git tracks), and reports the bytes freed (approximate: a
  hard-linked file counts once per link).
- `DISK_BUDGET`: worktrees over `[limits] worktree_budget_gb` (default 25 GB) by drive's last walk, after it freed the idlest
  lanes' build folders. A lane's build folders also go at its verdict (reviewer) or PASS (builder).
- `drover reclaim` (#361) lists, per worktree idle 7 days, the build folders that would go (`target/`, `node_modules/`,
  `.next/`, `dist/`, `.cache/` and the fleet's `build_dirs`, only git-ignored ones) and their sizes, then asks; `--apply`
  skips the question. Idle means no live lane, no process with its cwd there, HEAD not moved and no changed or
  untracked file modified in 7 days. Sources, branches and worktree registrations stay. Sizes are logical: an
  APFS-cloned folder shares blocks, so freeing it may save less. #1215: it also sweeps each enabled remote host's
  finished lanes' build and scratch folders, a lane no longer running in shed included (never a live lane's); a verdict frees its reviewer's scratch folder.
- `LANE_LIVE`: a live agent is in the worktree, or the log has an unfinished lane.
- `WORKTREE_DIRTY`: unpushed or uncommitted work from no known lane.
- `SERIAL_BUSY`, `CAP`, `MCP_FORBIDDEN`, `MERGED`: see `drover help --agent`.

A fleet's `env` table is exported into every lane's shell. #630: every lane gets its own Cargo build folder,
`CARGO_TARGET_DIR=<root>/.build/<agent>` under its workspace root (docs/hosts.md). At spawn it is cloned
copy-on-write (`cp -c`, `--reflink=always`; a `build-cloned` event with `from`) from the newest build folder of the
same fleet's lanes on that root, never across volumes, so a new lane skips a full rebuild and lanes never wait on
each other's cargo lock. #1480: on a `[hosts]` machine the clone runs there, in one ssh call (`cp -cR` on macOS,
`cp -R --reflink=always` elsewhere); with no sibling nothing is logged, and a filesystem without reflinks (ext4)
skips with the error in `build-cloned.errors`, never a full copy. A fleet's own `CARGO_TARGET_DIR` in `env`
or `shared_cache` (#361) replaces it. Over `[limits] build_budget_gb` (default 20), drive's pass removes the
build folders' profiles (`<agent>/debug`, ...), oldest first, whose `.cargo-lock` no cargo holds, never one
building nor a live lane's: on the main host
(and in `reclaim`) directly, on each `[hosts]` machine over ssh in every tick (there `lsof` tells a building one and its shed the live ones), one `builds: <host>:` line each with its total and what it freed, or its error. A host short
of disk (`DISK_LOW`) first loses every such profile, then its disk is read again. #536: each lane also gets a private scratch
dir, `DROVER_SCRATCH_TARGET=<root>/.build/.scratch/<agent>`, for second builds and checkouts (base, staging, head);
the hook refuses a `CARGO_TARGET_DIR` or `--target-dir` outside `<root>/.build` (`REFUSED SCRATCH_TARGET`). Scratch dirs
count toward `build_budget_gb` (oldest go first); drive deletes a lane's once it ended. #651: on every enabled host
(remote ones in the tick, before their trim, even past its budget; `[limits] sweep_remote = false` keeps it to this one) a scratch dir stays only
while its agent is working, or its agent still runs in shed and it is an open issue's latest lane of its role or no spawn names it yet;
a lane no longer running in shed counts as finished, verdict or not (#1215), so its build folder goes too.
Under `min_free_gb` every non-working lane's goes, largest first; each removal is logged with host, size and reason. `drover clean` lists the Cargo folders lanes left in `[limits] stray_roots`
(`/tmp`, `$TMPDIR`) untouched a day, and with `--yes` removes them and each removed worktree's Claude temp dir. Node fleets: pnpm's store is
already shared; for npm set `npm_config_cache`. #1641: `cache_size = "50G"` in this host's `[hosts.<name>]` entry
gives each lane started here `RUSTC_WRAPPER=sccache`, `SCCACHE_DIR=<root>/.cache/sccache`,
`SCCACHE_CACHE_SIZE=<cache_size>` and `npm_config_store_dir=<root>/.cache/pnpm`, so the host's lanes build each
dependency once; a lane's own crates stay in its `.build/<agent>`. The host setup recipe installs sccache; unset, no
lane gets them. The tools trim themselves: `reclaim`, `clean` and doctor print `shared cache <dir>: <GB> (kept)`
and never delete it. A fleet's `env` or `shared_cache` key of the same name wins. `clone_build = ["target"]` (or `["node_modules"]`) APFS-clones (`cp -cR`) each listed
folder of the local checkout into a new lane (never a reopened one) when its lockfile matches: `Cargo.lock`
for `target`, `package-lock.json` and `pnpm-lock.yaml` for `node_modules`; other names are logged and skipped.
It runs beside the kick once the lane has started, never over a folder the lane already has, and logs
`build-cloned {dirs, secs, errors}`. With no clone available (another volume; Linux without reflinks) it skips
and logs the error, never a full copy. The worktree budget and Host disk row count blocks used (`du -k`), not
apparent size. `clone_node_modules = true` is the old spelling of `clone_build = ["node_modules"]`, kept for
one release.

#1478: a Swift fleet sets `swift_build = true` in `[fleet.<n>]` (off by default: other fleets get no Swift folders and no
new env). Each of its lanes then also gets `DROVER_DERIVED_DATA=<root>/.build/<agent>/xcode` and
`DROVER_SWIFT_SCRATCH=<root>/.build/<agent>/swiftpm`, and its brief tells it to build with
`xcodebuild -derivedDataPath "$DROVER_DERIVED_DATA"` and `swift build --scratch-path "$DROVER_SWIFT_SCRATCH"`.
Both sit in the lane's own build folder, so the spawn's copy-on-write clone of the newest sibling carries them warm
(`build-cloned` lists them; no sibling, nothing is cloned), `build_budget_gb` trims them oldest first and the sweep
removes them with the lane's. A folder is never trimmed while xcodebuild holds its build database
(`Build/Intermediates.noindex/XCBuildData/build.db`) or SwiftPM its `.lock`; on a `[hosts]` machine `lsof` tells.
The hook refuses a `-derivedDataPath` or `--scratch-path` outside `<root>/.build` (`REFUSED SCRATCH_TARGET`).

#630: a repo's optional `.drover/worktree-include` lists ignored files (one path per line, `#` comments), such as
local env templates or tool config, that each new lane's worktree gets copied from the main checkout when missing.
Never secrets: those stay in `secret:` env. `drover status`, the lane sheet and `/api/v1/lanes` (its `unsaved` list:
`root`, `dirty`, `unpushed` per lane) show each lane's unsaved work, `+<uncommitted files> ⇡<unpushed commits>`, before a kill, supersede
or eject.

Every lane gets `DROVER_PORT` (#187): a stable hash of `<fleet>/<branch>` into the fleet's `port_range`
(default `"41000-41999"`), the next free one if a lane of an open issue already holds it; the spawn event
logs it and the Lanes tab shows it (`:41234`). Lanes start dev servers on it, never a fixed port; `env`
values expand it: `[fleet.web.env] PORT = "$DROVER_PORT"`. `scripts/ui-audit.sh` serves on it when set.

`sandbox = "srt"` in `[fleet.<n>]` (#782, a spike, off by default) starts the lane's agent as `srt --settings <worktree>.srt.json <harness …>`:
spawn writes those rules (worktree, build and its own temp writable; `~` read-denied but toolchains, its repo and the agent's own
folders; the secrets folder denied; network through srt's proxy to an allowlist). Profile, proofs and numbers: docs/spikes/lane-sandbox.md.

`drover close <agent>` closes the terminal of a shed agent that is not a drover lane (by name or pane id),
never touching files or worktrees, and logs a passive `closed-other`; a logged lane is refused `IS_LANE`
(use stop, kill or clean). `drover stop <agent>` sends Esc (logs `stop`). `drover kill <agent>` prints what is lost
(state, unpushed commits, dirty files, open PR); with `--yes` it commits dirty files as `wip: kill
snapshot`, pushes the branch to `wip/<agent>-<yyyymmdd-hhmm>` (never main/staging, never force;
refused `SECRET_IN_WIP` when the diff has a secret-looking value), closes the pane and logs
`lane-killed`. The worktree stays; a killed impl goes back
to `queued` (or `fix(n)`), a killed review to the state before it, both re-spawnable.
#646: every wip snapshot (kill, supersede, clean's auto-save, a cloud stop) stages tracked changes and new source
files only. It always skips `target*/`, `node_modules/` and `.lane-logs/` (whatever `.gitignore` says), #1367: an untracked
`*.log` (lane scratch: it neither counts as dirty nor is refused `SECRET_IN_WIP`; it stays in the worktree), any file over
5 MB, and the largest files once the snapshot passes 50 MB. The skipped files are listed on the kill output and in
the wip commit message, never committed. `drover wip prune <fleet> --dry-run` lists origin's `wip/*` branches
with the size each adds over the integration branch, its issue's state and whether it carries build output, after
the counts per issue. Without `--dry-run` (the owner's, through a run card) it deletes on origin those of a merged or
closed issue but the newest per issue and role (#779), and that one too if it carries build output; an open issue's
stay. `drover clean` lists the same, and deletes them with `--yes`: a branch delete, never a history rewrite or force-push. A drover-made clone (`hosts bootstrap`, a lane's
first use of a host) takes the integration branch only, and its later fetches skip `wip/*`.
`drover clean <fleet> --lane <agent>` applies `clean`'s safety to one lane and refuses
`UNSAFE_CLEAN` with the reason. #605: a merged or closed issue's lane whose agent is not `working` for
`[limits] idle_merged_minutes` (default 15) has its pane closed by `clean` (and so by drive's pass) the `kill --yes`
way, logged `lane-closed` with `why: merged-idle`, then its worktree removed; the dry run says `would close`. A
`SECRET_IN_WIP` or failed snapshot keeps the pane, one `lane-kept-dirty`, never retried; drive's own restart
refused `SECRET_IN_WIP` is one owner card naming the file and line. A working agent, an open
issue's lane, an orchestrator or any other agent in the folder stays `keep …: live agent`.

`--supersede` kills the old agent and logs it. `drover adopt` records a lane by hand.

Harness: `claude` runs Claude Code as an ACP session with the synced profile folder (#1726: the only one).
`drover set <fleet> <role> --harness <name>` switches one role.
