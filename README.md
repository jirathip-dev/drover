# herdsman

A Rust CLI that drives a fleet of coding agents over [herdr](https://herdr.dev). It works a GitHub
issue queue through build → review → merge on its own, with hard gates, host-load admission and a
live phone board with web push. Harness-neutral (Claude Code, Hermes, dsh).

## Why

If the rules live in a prompt, sooner or later an agent talks its way past them. herdsman keeps the
state in an append-only event log and owns every step that matters: spawn, verdict, merge. Agents act
only through it, and it refuses anything that breaks a rule.

- **It drives the queue itself.** `herdsman drive` does one pass (start the next issue, review a pushed
  head, fix a FAIL, merge a PASS); `herdsman timer install` runs it every few minutes. No AI orchestrator.
- **The pipeline is configurable.** The default is impl → rev → gates → merge; yours is validated as a
  full loop when the config loads and drawn on the board.
- **A phone board.** `herdsman serve` is a PWA: queue, lanes, host, events, and web push for what needs you.
  Frozen for features (#552): bug fixes only; new UI goes to cooee and cooee-drover, over `/api/v1` (see below).

## How it fits

```
cooee  →  cooee-drover  →  herdsman /api/v1
(app)     (tabs)           (engine)
```

Calls go one way, left to right; nothing on the right depends on what is to its left.

- **herdsman** (this repo) is the engine: queue, drive, lanes, reviews, gates, merge. It is to be renamed
  drover (#456).
- **[cooee](https://github.com/jirathip-dev/cooee)** is the app: chats, and shed sessions, which include the
  lanes as Workflow chats (cooee#33).
- **[cooee-drover](https://github.com/jirathip-dev/cooee-drover)** is cooee's Queue, Asks and Fleets tabs. They
  read and act through herdsman's `/api/v1` and reimplement nothing.
- The old board (`herdsman serve`) is frozen (#552) and retires once #553's parity rows pass on the owner's phone.

A new Queue-tab feature's data comes from herdsman's `/api/v1`, so herdsman changes first: it adds the field to
`/api/v1` (with its fixture in `crates/cli/tests/api_v1_contract.rs`), then cooee-drover's tab reads it. A
consumer never derives engine state itself ([docs/serve.md](docs/serve.md#json-api-v1-265)).

## Quick start
```sh
cargo install --path crates/cli
mkdir -p ~/.config/herdsman
cat > ~/.config/herdsman/fleet.toml <<'EOF'
[fleet.app]
repo = "you/app"
local = "~/src/app"
integration = "staging"

[fleet.app.role.impl]
harness = "claude"
profile = "lane-impl"

[fleet.app.role.rev]
harness = "claude"
profile = "lane-rev"
EOF
herdsman profiles sync                # per-role Claude Code folders and the lane hook
herdsman spawn app 12 impl --dry-run  # print the plan; drop --dry-run to launch it
herdsman status
herdsman timer install                # the drive loop, every few minutes
herdsman serve --addr 127.0.0.1:7777  # phone page at http://127.0.0.1:7777
```
No `pipeline` table means the default pipeline. Pick the board's look with `[serve] theme = "kitchen"`.
Full setup, from install to the board on your phone over Tailscale: [docs/setup.md](docs/setup.md).

## Commands

| Command | What it does |
|---|---|
| `drive` / `timer` / `tick` | one pass of the loop; the job that runs it; check, then drive |
| `queue` / `next` / `pause` / `resume` | ordered work queue; the first issue ready; hold a fleet |
| `spawn` / `adopt` | worktree + herdr pane + brief, launch the role's harness; record a running lane |
| `verdict` / `merge` | a review verdict pinned to the exact head sha; merge if every gate passes |
| `status` / `board` / `waiting` / `serve` | per-issue state; compact board; what waits on whom; phone page |
| `brief` / `pr` / `wait` | cached issue spec; cached PR and CI; wait for CI or merge |
| `check` / `ask` / `answer` / `asks` | alerts; ask for a decision, answer it, list the open ones |
| `clean` / `reclaim` / `stop` / `kill` / `close` | reclaim done lanes; idle lanes' build folders; Esc an agent; close its pane, unpushed work to `wip/` first |
| `set` / `profiles` / `accounts` / `hosts` | switch a role's model; per-role folders and hooks; accounts; hosts |
| `setup` / `doctor` / `protection` | install the jobs; host check; branch protection on? |
| `help --agent` | the usage doc agents get; refusals exit 3 with `REFUSED <CODE>: why` |

## Rules it enforces

- **Exact-head review.** `merge` needs a PASS verdict and green CI for the PR's current head sha. A new
  commit makes the old verdict stale.
- **Round limit.** At most 3 review rounds (`[limits].review_rounds`), then `needs-human`.
- **Host-load admission.** `spawn` refuses `HOST_BUSY` under memory pressure or high load, and
  `DISK_LOW`, `MEM_LOW` or `DISK_BUDGET` when space runs out.
- **Protected branches.** Merges go only to the integration branch, never main.
- **The lane hook.** Every lane runs a pre-tool hook that blocks merges, issue and PR comments, and
  pushes to protected branches. Reviewers are read-only.

## Pipeline

Stages, their kinds and where each outcome goes: [docs/pipeline.md](docs/pipeline.md).

## Themes

The Herd's look is a folder `themes/<name>/` (sprites, scene, words) next to fleet.toml; see
[docs/serve.md](docs/serve.md#themes-172).

## Hosts

Lanes on more than one machine, placed by tags and load: [docs/hosts.md](docs/hosts.md).

## Disk

`[limits] worktree_budget_gb` caps the worktrees; over it, drive frees the idlest lanes' build folders
first; `herdsman reclaim` previews, then frees, those of worktrees idle 7 days ([docs/lanes.md](docs/lanes.md)).

## Config

`~/.config/herdsman/fleet.toml` (or `FLEET_CONFIG`); every key is in [fleet.example.toml](fleet.example.toml).
More: [architecture](docs/architecture.md), [lanes and hooks](docs/lanes.md), [dsh](docs/dsh.md),
[phone page](docs/serve.md), [providers and keys](docs/providers.md).

## License

MIT, see [LICENSE](LICENSE).
