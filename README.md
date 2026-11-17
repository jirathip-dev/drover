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
- Threads: a tap or push opens the thread, `#threads/<id>` with back, and config, secret and run cards in a thread
  (#1088); typed text, focus and scroll survive repaints, with per-thread drafts (#1084).
- Threads at parity with the old chat: reading (#1086), the composer (#1087), two panes from 1000 px (#1118) and
  an edge swipe back (#1170).
- Queue counts and repo chips (#1089), a row tap always opens the issue sheet (#1090) with PASS/FAIL per round
  (#1238), and done today opens today's merges (#1172).
- The Herd scene back on the page (#1092).
- Every #553 parity row names its test in `docs/parity.md`; CI fails a dropped one without the owner's q-number (#1094).
- A held port hands over to the installed build; a stale or looping serve alerts (#1081).

**cooee plugin**
- Shed's lane states on the board and in the plugin (#983); each orchestrator as a cooee chat profile (#1057); run
  cards, config proposals and secret requests in a cooee chat (#1066).

**Merge, history and rules**
- Squash merges carrying their why notes (#1097); one history and issue convention by default (#1101).
- Design reviews and one more round from the owner's typed words (#828); a typed `#N approve` approves (#1080).
- SLO promises a known hold supersedes are void, not missed (#1093); one log read per pass (#832).
- Banned APIs (unwrap, panics, indexing, `as` casts) fail CI (#809).
- `drover compact`: a one-time fresh start of the history, with an owner card and undo (#1121).

## Next

The open queue, by area.

**The page and its screens**
- A Hosts screen on the page ([#1091](https://github.com/jirathip-dev/herdsman/issues/1091)); the remote-host token's state on it ([#1131](https://github.com/jirathip-dev/herdsman/issues/1131)).
- [later] on owner cards ([#1248](https://github.com/jirathip-dev/herdsman/issues/1248)); model and effort picks as dropdowns ([#1182](https://github.com/jirathip-dev/herdsman/issues/1182)).
- An answered ask card leaves at once ([#1185](https://github.com/jirathip-dev/herdsman/issues/1185)); a sent message shows once ([#1217](https://github.com/jirathip-dev/herdsman/issues/1217)); a new message keeps the
  composer's focus ([#1219](https://github.com/jirathip-dev/herdsman/issues/1219)).

**Drive, merge and gates**
- Closed issues and dropped holds leave the Queue at once ([#1025](https://github.com/jirathip-dev/herdsman/issues/1025)); a per-fleet cap on PRs in flight ([#1129](https://github.com/jirathip-dev/herdsman/issues/1129)).
- A faster pass: no worktree walk each minute ([#949](https://github.com/jirathip-dev/herdsman/issues/949)), no per-issue log scan ([#954](https://github.com/jirathip-dev/herdsman/issues/954)), a host sample with no
  sleep ([#951](https://github.com/jirathip-dev/herdsman/issues/951)).
- A stalled lane restarts, a long load wait moves host ([#1216](https://github.com/jirathip-dev/herdsman/issues/1216)); remote scratch is swept ([#1215](https://github.com/jirathip-dev/herdsman/issues/1215)).
- Proxy lanes' sign-out advice ([#1212](https://github.com/jirathip-dev/herdsman/issues/1212)); a `Repro: verify` PASS on a fleet with no verify role ([#1224](https://github.com/jirathip-dev/herdsman/issues/1224)).
- `drover compact` as a kept feature ([#1202](https://github.com/jirathip-dev/herdsman/issues/1202)).

**Hosts**
- Remote drover from a prebuilt Linux binary ([#1165](https://github.com/jirathip-dev/herdsman/issues/1165)).

**Rules, history and roles**
- Legacy history comments stripped ([#1098](https://github.com/jirathip-dev/herdsman/issues/1098)); a per-fleet git baseline ([#1099](https://github.com/jirathip-dev/herdsman/issues/1099)); a triage lane ([#1100](https://github.com/jirathip-dev/herdsman/issues/1100)).
- Simulation: gate-death scenarios ([#959](https://github.com/jirathip-dev/herdsman/issues/959)) and a timing-robust hunt test ([#1259](https://github.com/jirathip-dev/herdsman/issues/1259)).
