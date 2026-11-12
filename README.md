# herdsman (becoming drover)

A Rust CLI that runs a fleet of coding-agent lanes (impl, rev, verify, design, architect) over a GitHub issue queue.
Each issue goes build → review → merge through hard gates. herdsman keeps the state in an append-only event log and
owns spawn, verdict and merge; agents act only through it, and it refuses anything that breaks a rule. Owners talk to
it through orchestrator threads, in cooee or on the phone board.

```
cooee (app)  →  herdsman (Drover): /api/v1 and its screens  →  drive, lanes, gates, merge
```

## Today

Shipped on `staging`. Each line names the PR (or file) that proves it.

**Drive and gates**
- `herdsman drive` does one pass: start the next issue, review a pushed head, fix a FAIL, merge a PASS; `herdsman
  timer install` runs it every few minutes (#135, #213).
- Merge needs a PASS verdict and green CI at the PR's exact head sha, at most 3 review rounds, never main (`crates/core/src/gate.rs`).
- The lane hook blocks merges, comments and protected pushes; reviewers are read-only (`hooks/`, #848).
- Serial path groups (#306), the queue-wide line cap `herdsman cap` (#671), and arch-check from `.herdsman/arch.toml`
  (#758, #772).
- `--repro`, `--perf` and `--refactor` playbooks (#758); impl lanes end each turn with `herdsman turn` (#591).
- Each lane commit carries a why note: `herdsman why` (#691).
- A deterministic simulation of concurrent passes: `just sim` (#695, #697).

**Roles**
- impl and rev, on different providers if you like (#751).
- verify: drives the real app from `.herdsman/verify.md` after rev's PASS (#469, #765).
- design (UI mockups) and architect (ADR) run before impl, owner-approved (#448, #760).
- lesson: a weekly lane turns repeated review findings into checks, shadow first (#449, #586, #756).

**Lane runtime**
- herdr panes (the default), shed terminals, or shed-acp ACP sessions (#390, #480, #590); see
  [runtimes](docs/runtimes.md).
- Lane state comes from herdr's and shed's event streams (#585).
- Every external call is time-limited by one runner (#763).

**Orchestrator threads, asks and run cards**
- A thread per repo, with its orchestrator agent; many orchestrators allowed (#385, #811).
- A lane's ask goes to its repo's orchestrator first; owner topics wait for the owner (#551).
- Run cards: the exact command, approved once by the owner, run in the background (#587, #841).
- Thread handoff (#421), `config propose` cards (#400), design review answers (#828).

**Hosts and CI runners**
- Lanes on many machines, placed by tags and load (#211, #564); workspace roots (#645).
- Cloud hosts with idle stop and a spend budget (#280); shed-acp hosts with no herdr (#705).
- A host offline 15 min gets its lanes restarted on the main host (#847).
- A never-started needs-human lane gets [start lane] (#858).
- Owner-approved CI runner VMs for orchestrators (#596).

**Accounts, providers and secrets**
- Lanes sign in per account through config folders; herdsman keeps no tokens (#698, #795).
- Fallback to the next harness or account on a usage limit (#311).
- Providers: Claude Code, Codex, Gemini, Hermes, dsh, each one module (#836, `crates/core/src/providers/`).
- Secrets are 0600 files on the main host; no secret store, no Keychain read (#767).

**API and apps**
- `/api/v1`: the read and act API every screen uses, pinned by a contract test (#589, #583, #797).
- Drover in cooee: herdsman serves the plugin itself, `cooee-plugin.toml` and `/screens/` JSON blocks (#822).
- An orchestrator thread can live in cooee (#690).
- The lifeboat page at `/api/v1/lifeboat` for when cooee is down (#757).
- The old phone board (`herdsman serve`) is frozen to bug fixes (#552, `.github/workflows/board-freeze.yml`).

## In progress / next

**Rename**
- herdsman → drover: crates, binary, paths, env, launchd, hooks, docs ([#456](https://github.com/jirathip-dev/herdsman/issues/456)).

**Runtime**
- Retire herdr: shed is the only lane runtime ([#721](https://github.com/jirathip-dev/herdsman/issues/721)); one fake shed server for tests ([#771](https://github.com/jirathip-dev/herdsman/issues/771)).
- Sandbox every lane with srt, go/no-go ([#782](https://github.com/jirathip-dev/herdsman/issues/782)).

**Design recipes**
- The design medium from the issue, HTML by default ([#868](https://github.com/jirathip-dev/herdsman/issues/868)).
- A design pushed under the named path raises the ask ([#872](https://github.com/jirathip-dev/herdsman/issues/872)).
- A per-repo design recipe, `.herdsman/design.md` ([#873](https://github.com/jirathip-dev/herdsman/issues/873)).

**cooee and the old board**
- Parity list: every board and chat feature moves to cooee; then the board retires ([#553](https://github.com/jirathip-dev/herdsman/issues/553), [#746](https://github.com/jirathip-dev/herdsman/issues/746)).
- Non-fleet shed sessions in the Lanes tab ([#611](https://github.com/jirathip-dev/herdsman/issues/611)).
- One live index, fast `/api/v1/state` ([#859](https://github.com/jirathip-dev/herdsman/issues/859)).

**Drive robustness**
- An unreachable disabled host no longer stalls the tick ([#877](https://github.com/jirathip-dev/herdsman/issues/877)); skip a dead root ([#776](https://github.com/jirathip-dev/herdsman/issues/776)).
- Fleet SLOs and a self-heal ladder ([#855](https://github.com/jirathip-dev/herdsman/issues/855)); every wait a typed promise with a deadline ([#854](https://github.com/jirathip-dev/herdsman/issues/854)).
- Thread files from a lane's worktree ([#833](https://github.com/jirathip-dev/herdsman/issues/833)).

**Rules and checks**
- Comment rule, banned APIs, parity with cooee's lints ([#807](https://github.com/jirathip-dev/herdsman/issues/807), [#809](https://github.com/jirathip-dev/herdsman/issues/809), [#808](https://github.com/jirathip-dev/herdsman/issues/808)).
- Every core timing a `[limits]` key ([#844](https://github.com/jirathip-dev/herdsman/issues/844)); the workflow diagram from code ([#845](https://github.com/jirathip-dev/herdsman/issues/845)).

**Simulation**
- One sim scenario per past bug ([#856](https://github.com/jirathip-dev/herdsman/issues/856)); replay a real week ([#686](https://github.com/jirathip-dev/herdsman/issues/686)); what-if config runs ([#687](https://github.com/jirathip-dev/herdsman/issues/687)).

**Then**
- A full freeze: fixes only, the whole repo ([#559](https://github.com/jirathip-dev/herdsman/issues/559)).

## Naming

In #456 herdsman becomes **drover**, alongside cooee (the app) and shed (the runtime). The binary is `drover`; a
`herdsman` alias stays for one release and prints a rename notice. `drover migrate` moves `~/.config/herdsman`,
`~/.local/state/herdsman` and `~/.local/share/herdsman`. `HERDSMAN_*` env and `.herdsman/` are still read for one
release; the new names win. Fleet, lane and event names stay.

## Setup

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
herdsman profiles sync                # per-role profiles and the lane hook
herdsman spawn app 12 impl --dry-run  # print the plan; drop --dry-run to launch it
herdsman status
herdsman timer install                # the drive loop, every 5 minutes
herdsman serve --addr 127.0.0.1:7777  # /api/v1, cooee's screens and the old board
```

`herdsman setup` also installs `herdsman self-update`, every 5 minutes. It installs the head of `[self_update] branch`
(default `main`, which moves only when the owner promotes staging; `branch = "staging"` suits a dev box) once its CI is
green. The running serve hands its sockets (the board and the webhook port) to the new binary without dropping a
request: the page reconnects and resumes from its last event. `[self_update] staging_serve = true` adds
`dev.herdsman.serve-staging`, a read-only serve of the green staging head from its own install root on
`staging_addr` (default `127.0.0.1:7787`), to try a change before promoting it. Detail:
[setup](docs/setup.md#self-update-and-the-daily-clean-557).

| Command | What it does |
|---|---|
| `drive` / `timer` / `tick` | one pass; the job that runs it; check, then drive |
| `queue` / `next` / `pause` / `resume` | the ordered queue; the first ready issue; hold a fleet |
| `spawn` / `adopt` / `turn` | start a lane; record a running one; a lane's end of turn |
| `verdict` / `merge` / `cap` | a verdict pinned to the head sha; merge if every gate passes; the line cap |
| `status` / `board` / `waiting` / `serve` | per-issue state; compact board; who waits on whom; the server |
| `ask` / `answer` / `asks` / `thread` / `card` | decisions; orchestrator threads; run cards |
| `check` / `clean` / `reclaim` / `kill` | alerts; reclaim done lanes and build folders; stop a lane, work saved |
| `profiles` / `accounts` / `hosts` / `secret` | per-role folders and hooks; accounts; hosts; secret files |
| `setup` / `doctor` / `why` / `lesson` | install the jobs; host check; a line's reasoning; lesson checks |

Detail: [setup](docs/setup.md), [config](docs/config.md), [pipeline](docs/pipeline.md), [lanes and hooks](docs/lanes.md),
[hosts](docs/hosts.md), [accounts](docs/accounts.md), [orchestrator](docs/orchestrator.md), [serve and /api/v1](docs/serve.md),
[architecture](docs/architecture.md). Agents read `herdsman help --agent`.

## License

MIT, see [LICENSE](LICENSE).
