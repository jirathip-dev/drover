# drover

A Rust CLI that runs a fleet of coding-agent lanes (impl, rev, verify, design, architect) over a GitHub issue queue.
Each issue goes build → review → merge through hard gates. drover keeps the state in an append-only event log and
owns spawn, verdict and merge; agents act only through it, and it refuses anything that breaks a rule. Owners talk to
it through orchestrator threads, in cooee or on the phone page. Setup and detail: [setup](docs/setup.md),
[config](docs/config.md), [pipeline](docs/pipeline.md), [lanes](docs/lanes.md), [serve](docs/serve.md),
[architecture](docs/architecture.md); agents read `drover help --agent`.

## Today

Shipped on `staging`. Each line names the PR (or file) that proves it.

**Drive and gates**
- `drover drive` does one pass: start the next issue, review a pushed head, fix a FAIL, merge a PASS; `drover
  timer install` runs it every few minutes (#135, #213).
- Merge needs a PASS verdict and green CI at the PR's exact head sha, at most 3 review rounds, never main (`crates/core/src/gate.rs`).
- The lane hook blocks merges, comments and protected pushes; reviewers are read-only (`hooks/`, #848).
- Serial path groups (#306), the queue-wide line cap `drover cap` (#671), its history `drover cap log` (#1096), and
  arch-check from `.drover/arch.toml` (#758, #772).
- `--repro`, `--perf` and `--refactor` playbooks (#758); impl lanes end each turn with `drover turn` (#591).
- Each lane commit carries a why note: `drover why` (#691).
- A deterministic simulation of concurrent passes: `just sim` (#695, #697).

**Roles**
- impl and rev, on different providers if you like (#751).
- verify: drives the real app from `.drover/verify.md` after rev's PASS (#469, #765).
- design (UI mockups) and architect (ADR) run before impl, owner-approved (#448, #760).
- lesson: a weekly lane turns repeated review findings into checks, shadow first (#449, #586, #756).

**Lane runtime**
- shed terminals or shed-acp ACP sessions, set in `[runtime]` (#390, #480, #590, #721); lane state from shed's events (#585); see [runtimes](docs/runtimes.md).
- Every external call is time-limited by one runner (#763).

**Orchestrator threads, asks and run cards**
- A thread per repo, with its orchestrator agent; many orchestrators allowed (#385, #811).
- A lane's ask goes to its repo's orchestrator first; owner topics wait for the owner (#551).
- Run cards: the exact command, approved once by the owner, run in the background (#587, #841).
- Thread handoff (#421), `config propose` cards (#400), design review answers (#828).

**Hosts and CI runners**
- Lanes on many machines, placed by tags and load (#211, #564); workspace roots (#645).
- Cloud hosts with idle stop and a spend budget (#280); shed-acp hosts (#705).
- A host offline 15 min gets its lanes restarted on the main host (#847).
- A never-started needs-human lane gets [start lane] (#858).
- Owner-approved CI runner VMs for orchestrators (#596).

**Sign-in, providers and secrets**
- Lanes run on the host's own CLI login; drover keeps no tokens, and a usage limit holds that host's new lanes (#923).
- Providers: Claude Code, Codex, Gemini, Hermes, dsh, each one module (#836, `crates/core/src/providers/`).
- Secrets are 0600 files on the main host; no secret store, no Keychain read (#767).

**API and apps**
- `/api/v1`: the read and act API every screen uses, pinned by a contract test (#589, #583, #797); [serve health](docs/serve.md#serve-health-898) (#898).
- Drover in cooee: drover serves the plugin itself, `cooee-plugin.toml` and `/screens/` JSON blocks (#822).
- An orchestrator thread can live in cooee (#690).
- The lifeboat page at `/api/v1/lifeboat` for when cooee is down (#757).
- The phone page (`drover serve`) stays: it draws the same `/screens/` JSON cooee draws: Queue, Lanes, Events, the
  Fleets sheet and the cards (#927), plus a Threads tab of its own (#1072); its links open in a new tab (#1085).

## Next

The open queue, by area.

**The page and its screens**
- Threads at parity with the old chat: reading, composer, taps and cards in a thread ([#1086](https://github.com/jirathip-dev/herdsman/issues/1086), [#1087](https://github.com/jirathip-dev/herdsman/issues/1087), [#1088](https://github.com/jirathip-dev/herdsman/issues/1088)).
- Queue counts and chips, and a row tap that always opens the issue sheet ([#1089](https://github.com/jirathip-dev/herdsman/issues/1089), [#1090](https://github.com/jirathip-dev/herdsman/issues/1090)).
- A Hosts screen ([#1091](https://github.com/jirathip-dev/herdsman/issues/1091)) and the Herd scene ([#1092](https://github.com/jirathip-dev/herdsman/issues/1092)) back on the page.
- Typed text, focus and scroll survive repaints ([#1084](https://github.com/jirathip-dev/herdsman/issues/1084)).

**cooee plugin**
- Shed's lane states on the board and in the plugin ([#983](https://github.com/jirathip-dev/herdsman/issues/983)); each orchestrator as a cooee profile ([#1057](https://github.com/jirathip-dev/herdsman/issues/1057)).
- Run cards, config proposals and secret requests in a cooee chat ([#1066](https://github.com/jirathip-dev/herdsman/issues/1066)).

**Drive, merge and gates**
- Closed issues and dropped holds leave the Queue at once ([#1025](https://github.com/jirathip-dev/herdsman/issues/1025)); one log read per pass ([#832](https://github.com/jirathip-dev/herdsman/issues/832)).
- A handed-over serve runs the new binary ([#1081](https://github.com/jirathip-dev/herdsman/issues/1081)); SLO promises a known hold supersedes are not missed ([#1093](https://github.com/jirathip-dev/herdsman/issues/1093)).
- Squash merges carrying their why notes ([#1097](https://github.com/jirathip-dev/herdsman/issues/1097)).

**Orchestrators and asks**
- Design reviews and one more round from the owner's typed words ([#828](https://github.com/jirathip-dev/herdsman/issues/828)); a typed `#N approve` approves ([#1080](https://github.com/jirathip-dev/herdsman/issues/1080)).

**Rules and history**
- Banned APIs enforced in CI ([#809](https://github.com/jirathip-dev/herdsman/issues/809)); legacy history comments stripped ([#1098](https://github.com/jirathip-dev/herdsman/issues/1098)); parity rows are tests ([#1094](https://github.com/jirathip-dev/herdsman/issues/1094)).
- A per-fleet git baseline ([#1099](https://github.com/jirathip-dev/herdsman/issues/1099)), a triage lane ([#1100](https://github.com/jirathip-dev/herdsman/issues/1100)) and shared history conventions ([#1101](https://github.com/jirathip-dev/herdsman/issues/1101)).
