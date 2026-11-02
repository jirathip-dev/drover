# Runtime spike: herdr-core (`hc`) as an optional runtime (#197)

Spike, docs only: no `src/` change. The question: should herdsman support herdr-core (`hc`) next to herdr, and if
so, how? **Answer: no-go for now, with a trigger (end of page).**

Measured 2026-11-02 on the Mac Mini at herdsman `fe63c47` (staging), against `hc 0.1.16` (core `0.1.16`, OpenRPC
`1.3.2`, "Protocol generation 1") and `herdr 0.9.5`. hc `0.1.17` came out the same day
([CHANGELOG](https://github.com/herdrdev/herdr-core/blob/main/CHANGELOG.md)); it was not installed for this spike.
Every hc claim below is either real output from this host (a `$ hc …` block) or quoted with a link. In the output,
`<host>` is the machine name, `<ms>` an epoch-ms timestamp (the docs leak test bans long numbers), `$S` this spike's
scratch folder, and `…` a cut; everything else is verbatim. The test terminals (`spike197*`) were killed and removed
afterwards.

Sources quoted: [README](https://github.com/herdrdev/herdr-core/blob/main/README.md),
[docs/security.md](https://github.com/herdrdev/herdr-core/blob/main/docs/security.md),
[docs/machines.md](https://github.com/herdrdev/herdr-core/blob/main/docs/machines.md),
[CHANGELOG](https://github.com/herdrdev/herdr-core/blob/main/CHANGELOG.md),
[herdr-app README](https://github.com/herdrdev/herdr-app/blob/main/README.md), and `hc --skill` / `hc <cmd> --help` /
`hc schema` (0.1.16) on this host.

## What hc is, in one paragraph

`hc` runs programs in long-lived terminals and "draws nothing" (no panes, tabs or workspaces). A terminal has an
address (`t7`, a name, or `machine/t7`), free-form labels, an optional parent, and a status that only the program
reports. From `hc --skill`: *"Status comes from the program's OSC 7501 reports: `idle`, `working`, `done`, `blocked`,
`error`; plain programs report none. Do not guess status from screen text."* Errors are `{error: {kind, message,
hint}}` on stderr, with exit codes *"3 not_found, 7 needs_target, 9 blocked, 10 status_mismatch, 11 finished"*.

## 1. Call-by-call map

### Coverage (the AC's count)

`git grep -n 'herdr' src/` at `fe63c47` gives **195 lines**. Every one is listed in the [appendix](#appendix-every-grep-line)
with the row it maps to. They split as:

| Kind | Lines |
|---|---|
| A runtime call or its parsing (rows R1–R26) | 129 |
| Adapter plumbing: the program name, the runner seam (R0) | 11 |
| Comments and doc comments | 43 |
| `use` / `mod` lines | 9 |
| Other text (the CLI `about`, the worktree path `~/.herdr/worktrees`) | 3 |
| **Total** | **195** |

Recount: `git grep -n 'herdr' src/ | wc -l` → `195`. The 26 rows below are every distinct herdr command herdsman
runs: R1–R24 cover every function in `src/herdr.rs` (its argv builders and the helpers built on them), R25 is `--machine` in `src/hosts.rs`, and R26 the `update --handoff`
hint. (The issue's list named `agent get` and `workspace create|close`; herdsman calls none of them today. `pane get`
stands in for `agent get`, and `worktree create` creates the workspace.)

### The table

"Gap" means hc has no direct equivalent and the adapter must do it another way.

| # | herdr call (where it is built) | Used by | hc equivalent | Gap / note |
|---|---|---|---|---|
| R1 | `herdr status` (`herdr.rs:32`) | doctor, tick, Host tab | `hc server status --json` → `version` ([out](#server-status)); `hc --version` → `hc 0.1.16` | none. Parse a JSON field instead of herdr's YAML-ish text. |
| R2 | `herdr agent list` (`herdr.rs:37-55`) | check, drive, gate, spawn guard, serve, lanes, waiting, host | `hc ls --json` ([out](#ls)) | **Shape.** Rows have `address, agent, cwd, name, state, turn` and `status` only when reported. No `workspace_id`, `tab_id`, `terminal_title`, `completion_seq`. `agent` is the program name (`"sh"`), not a detected harness. Filter lanes with `--label`. |
| R3 | `herdr workspace list` (`herdr.rs:37`) | serve (workspace id → label) | none; `hc ls --label lane=<agent>` | **Gap**: no workspaces. A label carries the lane name. |
| R4 | `pane read <p> --source visible` (`herdr.rs:57-64`) | check (cap banner), serve, dsh/hermes kick | `hc read <t> --json` → `{"complete","how":"screen","text"}` ([out](#read)) | none (`--lines 0` is the default: the screen only). |
| R5 | `pane read --source recent --lines N --format ansi` (`herdr.rs:67-70`) | lane transcripts (#138), serve | `hc read <t> --lines N` | **Gap**: plain text only. The colours are stripped ([out](#read-strips-colour)); #138's coloured transcript would lose them. |
| R6 | `pane send-keys <p> <key>` (`herdr.rs:72-79`) | stop, serve menu answers, kicks | `hc keys <t> <keys…>` → `{"status":null,"turn":0}` ([out](#send-keys)) | Key names are lower-case (`enter`, `esc`): the adapter maps `Enter`/`Escape`. hc holds keys until *"nobody has typed for ~3 s"* (`hc schema`, `terminal.keys`). |
| R7 | `agent prompt <a> <text>` (no Enter) (`herdr.rs:81-88`) | DECISION delivery (`lib.rs:356`), hermes kick | `hc send <t> <text>` | **Shape**: `send` *"Paste text and press Enter"*: prompt + Enter is one call. `send` refuses a `blocked` terminal (exit 9) unless `--force`. |
| R8 | `pane send-text <p> <text>` (`herdr.rs:90-92`) | dsh reply and kick | `hc send <t> <text>` ([out](#send-keys)) | same as R7. |
| R9 | `reply` = R8 or R7, then R6 `Enter` (`herdr.rs:95-109`) | check (amendments, DECISION re-delivery), overlap, serve | `hc send <t> <text>` | simpler: one call. |
| R10 | `landed` = R17 for 20 s, retype once (`herdr.rs:112-115`) | DECISION delivery (#143) | `hc send --wait <t> <text>` (*"Then wait for the turn this send started"*; returns `not_started` when it hasn't begun, `hc --skill`) | not measured live (would cost a real turn). |
| R11 | `pane close <p>` (`herdr.rs:117-119`) | stop/kill/close, gate, clean, spawn retry | `hc kill <t>` then `hc rm <t>` ([out](#kill-and-rm)) | two calls; `kill` returns the finished terminal (`reason":"killed"`). |
| R12 | `failed_with(o, code)`: `{"error":{"code":…}}` (`herdr.rs:122-126`) | spawn (`agent_pane_busy`, `agent_name_taken`, `timeout`) | `{"error":{"kind":…}}` on stderr ([out](#errors)) | field is `kind`, not `code`. `name_taken` exists; `agent_pane_busy` cannot happen (each `run` is a new terminal). |
| R13 | `agent rename <p> <agent>` (`herdr.rs:128-130`) | dsh kick, spawn after a timeout, `adopt` | `hc label <t> lane=<agent>` ([out](#label-and-filter)) | **Gap**: no rename; the name is set once at `run --name` (*"unique among live terminals"*). Lanes would be found by label. |
| R14 | `pane get <p>` → `agent` non-null (`herdr.rs:132-140`) | spawn (timeout, adopt), dsh poll | `hc get <t> --json` → `status`, `reported`, `foreground.argv` ([out](#get)) | **Semantics**: hc *"does not … recognize agent brands"* (README). "A harness runs here" becomes "it reported status" (`reported:true`) or a `foreground.argv` match. |
| R15 | `pane run <p> <words…>` (`herdr.rs:143-145`) | lane env export (`spawn.rs:124`), dsh launch | `hc send <t> "<line>"` into a shell terminal | Works the same, but see R16: hc can start the harness directly. |
| R16 | `agent start <a> --kind <k> --pane <p> --timeout 90000 -- <args>` (`herdr.rs:148-153`) | claude and hermes launch | `hc run --bg --json --name <a> --cwd <wt> --label lane=<a> --no-parent [--outlive-parent] -- <cmd> <args>` ([out](#run)) | No `--kind` and no readiness timeout: `run --bg` returns at once; readiness = a reported `idle` (`hc wait`). `run` has no `--env` (only `exec` has), so env still goes through a shell line (R15) or `sh -c`. |
| R17 | `agent wait <a> --until working --until blocked --timeout ms` (`herdr.rs:156-168`) | claude kick confirm, hermes kick, R10 | none | **Gap**: `hc wait` waits until it is *not* working (*"any word but working"*, `hc schema`). The inverse is `send --wait`, or watching `hc events` for `working`. |
| R18 | `pane wait-output <p> --match X --source visible --timeout ms` (`herdr.rs:170-182`) | hermes kick (waits for `❯`) | `hc wait <t> --output X --timeout s` ([out](#wait)) | **Semantics**: hc matches only *"output printed after the wait started"*; herdr matches what is already visible. Read first (R4), then wait. |
| R19 | `worktree create --cwd --branch --base --path --label --no-focus` (`herdr.rs:184-200`) | spawn | `git worktree add -b <branch> <path> <base>` (local, or `hc exec --on <m>`), then R16 with `--cwd` | **Gap**: hc has no worktree or workspace commands (`hc --help`). herdsman runs git itself, as `drive.rs:552` already does for scratch trees. |
| R20 | `worktree open --cwd --path --label` (`herdr.rs:202-204`) | spawn (fix rounds reuse the tree) | nothing: the tree exists, R16 `--cwd` it | gap, but trivial. |
| R21 | `workspace rename <ws> <label>` (`herdr.rs:206-208`) | spawn (#194) | `hc label` | gap; not needed without workspaces. |
| R22 | `worktree remove --workspace <ws>` (`herdr.rs:211-213`) | clean, gate | `git worktree remove` | gap; `check.rs:659-661` already falls back to `git worktree remove`. |
| R23 | `pane list --workspace <ws>` (`herdr.rs:217`) | `free_pane` | `hc ls --label lane=<a> --json` ([out](#label-and-filter)) | no workspace; filter by label. |
| R24 | `pane split <p> --direction right --cwd --no-focus` (`herdr.rs:230-242`) | `free_pane` | `hc run --bg --cwd <wt>` (a new shell terminal) | no layout in hc; `free_pane` becomes "start a terminal". |
| R25 | `herdr --machine <m> <cmd>` (`hosts.rs:35`) | every call for a lane on another host (#157) | `run/exec/file … --on <m>`; every other command takes the address `<m>/t7` | see [Multi-host](#4-multi-host-157-on-herdr---machine-vs-hc-machine). |
| R26 | `herdr update --handoff` hint (`timer.rs:16`, `doctor.rs:25`) | Host tab, doctor | `hc update` (*"terminals keep running"*, `hc update --help`) | no handoff step; an update restarts the core and the holders keep the programs. |

### Measured output

#### server status
```
$ hc server status --json
{"core":{"id":"269e66bf2a8159446b448dcd39b07324","name":"<host>"},"finished":15,"holder":{"current":true,"instance":"cb06a0d691c6b11fcdf1314bc9cf6d0f","pid":48066,"terminals":1},"holders":[{"current":true,"instance":"cb06a0d691c6b11fcdf1314bc9cf6d0f","pid":48066,"terminals":1}],"installed_by":null,"live":1,"pid":47724,"uptime_ms":535791,"version":"0.1.16"}
exit=0
```

#### ls
One Claude Code terminal (it reports status) next to a plain `sh` (it does not):
```
$ hc ls --json
{"hidden_finished":15,"terminals":[{"address":"t17","agent":"claude-code","cwd":"$S/cc","name":"hs-cc-197","state":"live","status":"idle","turn":0}]}
$ hc run --bg --json --name spike197 --cwd $S/wt --label herdsman=spike --no-parent sh
{"address":"t18","agent":"sh","cwd":"$S/wt","name":"spike197","state":"live","turn":0}
```
For comparison, one row of `herdr agent list` (0.9.5):
```
{"agent":"claude","agent_status":"idle","completion_seq":112,"cwd":"…/issue-182","focused":false,"foreground_cwd":"…/issue-182","interactive_ready":true,"name":"herdsman-impl-182","pane_id":"w5:p1","revision":1,"state_change_seq":112,"tab_id":"w5:t1","terminal_id":"term_65f45daa27d8d5","terminal_title":"✳ herdsman · impl #182 · …","workspace_id":"w5"}
```

#### get
```
$ hc get t17 --json
{"address":"t17","argv":["claude"],"cols":200,"created_ms":<ms>,"cwd":"$S/cc","die_with_parent":false,"exit_code":null,"finished_ms":null,"foreground":{"argv":["claude"],"cwd":"$S/cc","name":"2.1.309","pid":50052},"identity":"ghostty","keep":false,"labels":{},"name":"hs-cc-197","notify":true,"parent":null,"pid":50052,"reason":null,"reported":true,"rm":false,"rows":120,"screen":"exact","signal":null,"state":"live","status":{"app":"claude-code","state":"idle"},"status_ms":<ms>,"template":null,"title":"Claude Code","turn":0}
$ hc get spike197 --json
{"address":"t18","argv":["sh"],…,"labels":{"herdsman":"spike"},"name":"spike197",…,"reported":false,…,"state":"live","status":null,"status_ms":null,"template":null,"title":"","turn":0}
```
Note: `ls` gives `status` as a string; `get` gives `{"app","state"}`.

#### run
See [ls](#ls). A second run with a live name:
```
$ hc run --bg --json --name spike197b --cwd $S/wt --no-parent sh
{"error":{"hint":"pick another name, or kill the one holding it","kind":"name_taken","message":"a live terminal is already named \"spike197b\""}}
exit=1
```

#### send, keys
```
$ hc send spike197 'echo READY-197' --json
{"started":null,"status":null,"turn":0}
$ hc keys spike197 ctrl+c --json
{"status":null,"turn":0}
$ hc interrupt spike197 --timeout 2 --json
{"key":"ctrl+c","outcome":"unknown"}
```

#### read
```
$ hc read spike197 --json
{"complete":true,"how":"screen","text":"sh-3.2$ echo READY-197\nREADY-197\nsh-3.2$"}
```

#### read strips colour
```
$ hc run --bg --json --name spike197c --cwd $S/wt --no-parent sh -c 'printf "\033[31mRED\033[0m\n"; sleep 30'
$ hc read spike197c --lines 5 --json
{"complete":true,"how":"history","text":"RED"}
```

#### wait
```
$ hc wait spike197 --output DONE-197 --timeout 3 --json
{"children":[],"ended":false,"exit_code":null,"reason":"timeout","signal":null,"status":null,"terminal":"t18","turn":0}
exit=0
$ hc wait t17 --timeout 2 --json
{"children":[],"ended":true,"exit_code":null,"reason":"status","signal":null,"status":"idle","terminal":"t17","turn":0}
$ hc send spike197b exit --json; hc wait spike197b --timeout 5 --json
{"children":[],"ended":true,"exit_code":0,"reason":"exit","signal":null,"status":null,"terminal":"t19","turn":0}
```
A timeout exits 0 with `"reason":"timeout"`: the adapter must read `reason`, not the exit code.

#### label and filter
```
$ hc label spike197 lane=herdsman-impl-197 --json
{…,"labels":{"herdsman":"spike","lane":"herdsman-impl-197"},"name":"spike197",…}
$ hc ls --label lane=herdsman-impl-197 --json
{"hidden_finished":0,"terminals":[{"address":"t18","agent":"sh","cwd":"$S/wt","name":"spike197","state":"live","turn":0}]}
```

#### errors
```
$ hc get nope197 --json
{"error":{"hint":"see `hc ls --all`","kind":"not_found","message":"no terminal \"nope197\""}}
exit=3
$ hc send spike197b 'echo hi' --json      # after it exited
{"error":{"hint":"hc read t19","kind":"finished","message":"t19 has finished"}}
exit=11
```

#### kill and rm
```
$ hc kill spike197 --json
{"address":"t18",…,"reason":"killed",…,"signal":1,"state":"finished",…}
$ hc rm spike197 --json
{"removed":"t18"}
```

#### exec (no terminal)
```
$ hc exec --json --cwd $S/wt git --version
{"code":0,"ms":9,"signal":null,"stderr":"","stdout":"Z2l0IHZlcnNpb24gMi41Ni4wCg==","timed_out":false,"truncated":false}
$ hc exec git --version
git version 2.56.0
```
`--json` stdout is base64. `exec` times out at *"default 60, at most 600"* seconds (`hc exec --help`), so a long
`git fetch` or `gh repo clone` on another host could not use it.

#### events
```
$ hc events --json --terminal t17      # first line: a snapshot of matching terminals
{"channel":null,"seq":16,"terminals":[{"address":"t17",…,"status":{"app":"claude-code","state":"idle"},…}]}
```
herdr has no event stream herdsman uses; `herdsman serve` polls `agent list` every 2 s (`serve.rs:641`). `hc events`
could replace that poll, but it is not needed for a first cut.

## 2. Adapter shape

**Setting:** `runtime = "herdr" | "hc"` in `[fleet.<name>]`, default `herdr`. herdsman checks it in `doctor` (is the
binary on PATH, does `hc server status` answer) and records it in each `spawn` event, so a lane keeps the runtime it
started on even if the setting changes later.

**Today `herdr.rs` is the only file that builds argv, but not the only file that knows herdr.** 27 lines outside it
read herdr's JSON (`["agent_status"]`, `["pane_id"]`, `["workspace_id"]`) in check, drive, gate, spawn, lanes, serve,
waiting and the dsh harness; the harnesses build herdr argv lists (`kick_cmds`) that `spawn --dry-run` prints; spawn
reads `result.workspace.workspace_id`; `hosts.rs` special-cases `herdr::BIN` for `--machine`. For `hc`, these move
behind one module:

```
src/runtime/mod.rs   trait Runtime + struct Agent { name, pane, status, lane, cwd } + runtime(cfg, fleet)
src/runtime/herdr.rs today's herdr.rs, its JSON mapped to Agent
src/runtime/hc.rs    the hc impl; git worktree through hosts::On
```

```rust
pub trait Runtime {
    fn agents(&self) -> Result<Vec<Agent>>;                 // R2/R3 (hc: ls --label herdsman=<fleet>)
    fn read(&self, pane: &str, lines: Option<u32>) -> Result<String>;   // R4/R5
    fn keys(&self, pane: &str, key: Key) -> Result<()>;     // R6
    fn reply(&self, a: &Agent, text: &str) -> Result<()>;   // R7-R9
    fn landed(&self, a: &Agent, text: &str) -> bool;        // R10/R17
    fn close(&self, pane: &str) -> Result<()>;              // R11
    fn worktree(&self, p: &Plan, reopen: bool) -> Result<Workspace>;    // R19-R21
    fn remove(&self, ws: &Workspace) -> Result<bool>;       // R22
    fn start(&self, p: &Plan, launch: Launch) -> Result<Started>;       // R12-R16, R23/R24
    fn plan(&self, p: &Plan) -> Vec<Vec<String>>;           // the dry-run lines
    fn version(&self) -> Result<String>;                    // R1/R26
}
```

`Status` stays a string with herdr's words so the 27 call sites change only from `a["agent_status"]` to `a.status`.
`Started`'s error enum replaces `failed_with` (R12). The harnesses keep their per-harness steps (settle, kick,
confirm) but ask the runtime for each step instead of building argv.

**Size estimate** (formatted lines, as `herdsman cap-check` counts them):

| Part | Lines |
|---|---|
| `runtime/mod.rs` (trait, `Agent`, selector, `Key`) | +70 |
| `runtime/herdr.rs` (today's 247, plus the JSON→`Agent` mapping) | +45 |
| `runtime/hc.rs` (the 24 operations, label mapping, git worktree, `kind` errors) | +220 |
| Call sites: 27 JSON reads → fields (about even), harness `kick_cmds` → steps, spawn's worktree block, `hosts.rs` wrap, config field | +40 |
| **src/ net** | **≈ +375** |
| Tests (a fake `Runtime`, hc JSON fixtures from the outputs above) | ≈ +200, outside `src/` |

The cap today: `src lines (formatted): 13970 / cap 15000` (`cargo run -q -- cap-check --config ci/cap.toml`). +375
fits, leaving about 650.

## 3. Status mapping

| herdr `agent_status` | hc OSC 7501 (`hc get` `.status.state`) | herdsman meaning |
|---|---|---|
| `working` | `working` | busy: no stall, no reap, gate leaves the reviewer alone |
| `idle` | `idle` | waiting for input |
| `done` | `done` | the turn ended |
| `blocked` | `blocked` | a person is needed (a permission or a question) |
| (none) | `error` | new in hc. Map to `blocked`: *"`error`: read why, then send or kill"* (`hc --skill`). |
| `unknown` | `null` (not reported) | no status. herdr 0.9 guesses from the screen for the kinds it knows; hc never does. |
| (no row) | no terminal / `state: finished` | gone: `lane-dead`, or `parked` when the tree is clean and pushed (#83) |

**What the code does today with a lane whose status is `unknown`** (this is what every non-reporting harness on hc
would get):

- **check, stall** (`check.rs:120`): fires once the lane has had no event for the stall time, because the status is
  not `working`. Correct enough: a lane that reports nothing cannot prove it is busy.
- **check, idle reap** (#223, `check.rs:112-113`): needs `done` or `idle`, so it never trims the lane's children.
  Safe.
- **check, DECISION re-delivery** (#143, `check.rs:175`): needs `idle` or `done`, so an undelivered DECISION is never
  typed again. A real loss.
- **gate, after a verdict** (`gate.rs:155`, `gate.rs:169-171`): "not `working`" counts as free, so it purges the
  builder's build folders and closes the reviewer's pane. That is safe only because the verdict is the reviewer's last
  act; a reviewer that writes `verdict.md` early and keeps going would be cut off.
- **drive** (`drive.rs:535`) and **waiting** (`waiting.rs:287`): "not `working`" = not busy; the lane shows as waiting.
- **spawn's duplicate guard** (`spawn.rs:505`): an old lane that is not `working` counts as stale, so `--supersede`
  logic treats it as finished.

**Per harness:**

- **Claude Code** reports OSC 7501: measured (`"status":{"app":"claude-code","state":"idle"}`, Claude Code 2.1.309).
  The `working`/`blocked` transitions were not measured (that needs a real turn).
- **Hermes**: not measured. hc says plain programs report none; if Hermes does not emit OSC 7501, its lanes get the
  `unknown` behaviour above, and its kick (R17: wait until `working`) has nothing to wait on.
- **dsh**: dsh-TUI reports to herdr today (`harness/dsh.rs:14`, `LABEL = "dsh-tui"`) and `dsh.rs:127` polls for it. On
  hc it would need to emit OSC 7501 instead; until then the same `unknown` behaviour applies, and its kick poll never
  succeeds.

**Rule for a fleet on `hc`:** every harness the fleet uses must report OSC 7501, checked by `doctor` (start it, wait
for `reported:true`). A harness that does not stays on `herdr`. No screen-scraped status in herdsman: that is the
thing hc deliberately does not do.

## 4. Multi-host: #157 on `herdr --machine` vs `hc machine`

#157 (`docs/hosts.md`) runs herdr commands for a remote lane as `herdr --machine <m> …` and everything else (git,
probes, brief, `verdict.md`, dirty/unpushed checks) as `ssh <m> …`, so the machine label must also be an SSH host name.

| | herdr 0.9.5 (#157) | hc 0.1.16 |
|---|---|---|
| Add a machine | `herdr machine add`, `herdr machine status` | `hc machine add user@host --name box` (`--install` copies hc there), `hc machine ls` → `{"machines":[]}` here |
| Address a remote lane | every command takes `--machine <m>` | `run`, `exec`, `file` take `--on <m>`; the others take `<m>/t7` (*"its address is `machine/t7`"*, `hc run --help`) |
| Non-runtime work (git, files) | a second channel: `ssh <m>` with `BatchMode=yes` | the same channel: `hc exec --on <m>` (60–600 s limit) and `hc file read|write|edit --on <m>` |
| One list of all lanes | one `agent list` per machine (`hosts::all_agents`) | the local core *"brings its terminals into the same list"* (machines.md); not measured, no machine added here |
| Lost connection | the lane keeps running on its host's herdr server | *"Losing your connection does not end it"* (machines.md) |
| Non-SSH transports | no | `--via '<cmd>'` (fly, Vercel sandbox) |

hc would remove #157's second channel (no SSH alias needed for git and files) and the per-machine list loop. It
would not remove `git worktree` over the wire, and `exec`'s 600 s ceiling rules it out for `gh repo clone` on a first
lane. A narrower step that needs no runtime switch: use `hc exec --on` / `hc file --on` for #157's ssh channel only.

## 5. Risks

- **A young 0.1.x API.** hc was created 2026-10-16 (`gh api repos/herdrdev/herdr-core`); the CHANGELOG lists 0.1.11 to
  0.1.17 between 2026-10-26 and 2026-11-02: seven releases in eight days. The ones read here are fixes and additions,
  no CLI removal, but they include behaviour changes herdsman would feel: *"Child terminals end when their parent ends
  by default"* (0.1.13). herdsman would start lanes with `--no-parent` (and `--outlive-parent` where a parent is set).
  Track: the CHANGELOG, `hc schema` (`info.version`), and the `hello` method's *"protocol generation"*.
- **Telemetry.** The herdr-app README: *"Usage telemetry is on by default in releases. Turn it off in Settings →
  Privacy."* The hc README, `docs/security.md` and `docs/machines.md` do not mention telemetry; the only outbound call
  they name is updates: *"The default release service is HTTPS at releases.herdr.dev."* herdsman would need only
  `hc`, never the app. That the core sends none was not verified at the network level.
- **Updates and running lanes.** *"Updates restart the core while holders keep the terminals running"*
  (security.md). The 0.1.17 entry *"On macOS, quitting Herdr no longer stops your agents … Agents that were already
  running before the update pick this up the next time they start"* shows that a lane's behaviour can depend on the
  version it started under. `herdsman tick` should not run `hc update` itself; the owner updates between lanes.
- **Two runtimes at once.** herdr 0.9.5 and hc 0.1.16 run side by side on the Mini today; lanes are on herdr, hc only
  shows the owner's app terminals. `hc ls` lists *"all live terminals, not just yours"* (`hc --skill`), so a fleet on
  hc must filter by its own label (`herdsman=<fleet>`) and never touch other terminals.
- **Trust.** *"There is no separate API password or per-client permission check"* (security.md), same as herdr's
  socket today. The account token herdsman exports into a lane's shell (R15) stays out of `hc get`'s `argv` only if it
  is typed into a shell, not passed to `hc run`.
- **Windows:** not needed; not looked at.
- **Leftover found:** a `claude` terminal `hs-cc-197` (t17) from an earlier attempt at this lane was live in hc; it is
  the Claude Code measured above, and it was killed and removed after the measurements.

## 6. Go / no-go

**No-go now.** herdr 0.9.5 does everything herdsman needs; hc would need herdsman to take over worktrees (R19–R22),
lose coloured transcripts (R5) and two waits (R17, R18 semantics), and it has no proof yet that Hermes and dsh report
status. The cost is ≈ +375 src lines out of about 1,030 left under the cap, for no new capability the owner asked
for.

**Go when all of these hold:**

1. hc ≥ 0.2, and the `hc schema` methods herdsman uses (`terminal.run/list/get/read/send/keys/wait/kill/rm/label`,
   `machine.exec`, `file.*`) are unchanged across two releases in a row (diff `hc schema` output);
2. every harness in use reports OSC 7501 (Claude Code does; Hermes and dsh to be measured);
3. a wait-until-working exists (or `send --wait` is measured to cover #143's `landed`), and `read` can return ANSI
   (or the owner accepts plain-text transcripts).

**Go earlier, narrowly, if** #157 on `herdr --machine` proves flaky: more than one `lane-dead` or failed remote spawn
a week traced to the ssh channel. Then the first step is only #157's second channel (`ssh <m>` → `hc exec/file --on
<m>`), with herdr kept as the runtime.

Revisit after #157 settles, before #156.

## Appendix: every grep line

`git grep -n 'herdr' src/` at `fe63c47`, 195 lines. "Row" is the table row above; R0 is adapter plumbing (the program
name and the runner seam); "—" is not a runtime call.

| Line | Row | Text |
|---|---|---|
| `src/check.rs:11` | — use/mod | `use crate::{gh, herdr};` |
| `src/check.rs:44` | R2 | `.map_err(\|e\| eprintln!("herdsman check: herdr agent list failed ({e:#}); skipping lane c` |
| `src/check.rs:91` | R4 | `let screen = herdr::read(r, p).unwrap_or_default();` |
| `src/check.rs:100` | R5 | `crate::lanes::roll(&f, &crate::lanes::body(&herdr::recent(r, p, "200").unwrap_or_default()` |
| `src/check.rs:106` | R9 | `if !seen && herdr::reply(r, dsh, lane, p, &line).is_ok() {` |
| `src/check.rs:148` | — comment/doc | `// #107: a herdr agent named like a lane ('<fleet>-<role>-<n>…') that no 'spawn' event log` |
| `src/check.rs:156` | R2 | `let d = json!({"key": name, "reason": format!("herdr agent {name} has no spawn event; spaw` |
| `src/check.rs:179` | R9 | `if quiet && herdr::reply(r, false, agent.unwrap_or_default(), pane, msg).is_ok() {` |
| `src/check.rs:284` | R11 | `None => drop(herdr::close(&on, pane)),` |
| `src/check.rs:306` | R2 | `let agents = crate::hosts::all_agents(cfg, r).context("cannot list herdr agents; refusing ` |
| `src/check.rs:391` | R11 | `let _ = herdr::close(&on, pane);` |
| `src/check.rs:434` | — comment/doc | `/// #186: the processes in worktrees of closed lanes (newest spawn not its issue's active ` |
| `src/check.rs:659` | — comment/doc | `/// herdr first; else 'git worktree remove' (never '--force') of the re-checked tree minus` |
| `src/check.rs:661` | R22 | `if !ws.is_empty() && herdr::worktree_remove(r, ws)? {` |
| `src/check.rs:698` | R6 | `herdr::keys(&On::of(r, Some(e)), e.str("pane").unwrap_or_default(), "Escape")?;` |
| `src/check.rs:707` | — comment/doc | `/// 'herdsman close': close a non-lane herdr agent's pane (by name or pane id); files stay` |
| `src/check.rs:709` | R2 | `let (events, agents) = (read(&ctx.state)?, herdr::agents(r)?);` |
| `src/check.rs:715` | R2 | `let pane = a.and_then(\|a\| a["pane_id"].as_str()).context("no such herdr agent")?;` |
| `src/check.rs:716` | R11 | `herdr::close(r, pane)?;` |
| `src/check.rs:790` | R11 | `herdr::close(&on, e.str("pane").unwrap_or_default())?;` |
| `src/cli.rs:20` | — text/path | `about = "State, gates and harness toggle over herdr",` |
| `src/cli.rs:52` | — comment/doc | `/// Create worktree + herdr pane, write .brief.md, launch the role's harness, verify the p` |
| `src/cli.rs:60` | — comment/doc | `/// Record a lane already running in a herdr pane (repair a spawn that was never logged)` |
| `src/cli.rs:65` | — comment/doc | `/// The herdr pane the harness runs in` |
| `src/cli.rs:188` | — comment/doc | `/// With --export: the inert demo, its herdr/gh/host reads canned in runner.json next to f` |
| `src/cli.rs:236` | — comment/doc | `/// Read-only host check: herdr, gh, config, accounts, harnesses, serve, phone, watchdog; ` |
| `src/cli.rs:258` | — comment/doc | `/// Close the pane of a herdr agent that is not a herdsman lane (IS_LANE for lanes); files` |
| `src/config.rs:28` | — comment/doc | `/// #157: one host: its herdr machine label (unset = this host), hand tags, worktree root,` |
| `src/doctor.rs:24` | R1 | `if let Some((client, server, older)) = crate::timer::herdr_versions(r) {` |
| `src/doctor.rs:25` | R26 | `let line = format!("herdr — client {client}, server {server} → herdr update --handoff");` |
| `src/doctor.rs:28` | R1 | `rows.push("✗ herdr — not on PATH, or no server → herdr".into());` |
| `src/drive.rs:552` | — comment/doc | `// A scratch 'git worktree', not a herdr one: no pane, no lane.` |
| `src/gate.rs:6` | — use/mod | `use crate::{check, gh, herdr};` |
| `src/gate.rs:156` | R2 | `if wt.is_dir() && herdr::agents(r).is_ok_and(\|a\| !a.iter().any(working)) {` |
| `src/gate.rs:168` | — comment/doc | `// A reviewer herdr still shows 'working' (or an unreadable agent list) is left alone.` |
| `src/gate.rs:169` | R2 | `let busy = herdr::agents(r).map_or(true, \|a\| {` |
| `src/gate.rs:183` | R11 | `.and_then(\|()\| herdr::close(r, pane).map(drop)),` |
| `src/gate.rs:557` | R2 | `if let Ok(a) = herdr::agents(r) {` |
| `src/harness/claude.rs:3` | — use/mod | `use crate::herdr;` |
| `src/harness/claude.rs:57` | — comment/doc | `// Display name = terminal title on the herdr board (else Claude titles it from the kick).` |
| `src/harness/claude.rs:66` | R16 | `herdr::start_argv(&p.agent, "claude", pane, a)` |
| `src/harness/claude.rs:71` | R17 | `vec![herdr::wait_working_argv(&p.agent, "20000")]` |
| `src/harness/claude.rs:75` | R17 | `Ok(herdr::exec(r, &self.kick_cmds(p, pane)[0])?.code == 0)` |
| `src/harness/dsh.rs:3` | — use/mod | `use crate::herdr;` |
| `src/harness/dsh.rs:14` | — comment/doc | `/// The agent label dsh-TUI reports to herdr.` |
| `src/harness/dsh.rs:73` | — comment/doc | `/// herdr has no 'dsh' kind: type the launch into the lane's shell.` |
| `src/harness/dsh.rs:76` | R15 | `herdr::run_argv(pane, &["dsh-tui", "--patch", &patch])` |
| `src/harness/dsh.rs:82` | R14 | `herdr::pane_get_argv(pane),` |
| `src/harness/dsh.rs:83` | R4 | `herdr::read_argv(pane),` |
| `src/harness/dsh.rs:84` | R6 | `herdr::keys_argv(pane, "esc"),` |
| `src/harness/dsh.rs:85` | R13 | `herdr::rename_argv(pane, &p.agent),` |
| `src/harness/dsh.rs:86` | R8 | `herdr::send_text_argv(pane, &p.kick),` |
| `src/harness/dsh.rs:87` | R14 | `herdr::pane_get_argv(pane),` |
| `src/harness/dsh.rs:88` | R6 | `herdr::keys_argv(pane, "enter"),` |
| `src/harness/dsh.rs:98` | R4 | `let screen = herdr::exec(r, &c[1])?.stdout;` |
| `src/harness/dsh.rs:100` | R6 | `herdr::exec(r, &c[2])?;` |
| `src/harness/dsh.rs:106` | R13 | `herdr::exec(r, &c[3])?;` |
| `src/harness/dsh.rs:109` | R4 | `if attempt == 0 \|\| !herdr::exec(r, &c[1])?.stdout.contains(&typed) {` |
| `src/harness/dsh.rs:110` | R8 | `herdr::exec(r, &c[4])?;` |
| `src/harness/dsh.rs:112` | R6 | `herdr::exec(r, &c[6])?;` |
| `src/harness/dsh.rs:124` | R14 | `let pane = herdr::json(r, get)` |
| `src/harness/hermes.rs:3` | — use/mod | `use crate::herdr;` |
| `src/harness/hermes.rs:44` | R16 | `herdr::start_argv(&p.agent, "hermes", pane, a)` |
| `src/harness/hermes.rs:50` | R18 | `herdr::wait_output_argv(pane, "❯", "60000"),` |
| `src/harness/hermes.rs:51` | R7 | `herdr::prompt_argv(&p.agent, &p.kick),` |
| `src/harness/hermes.rs:52` | R6 | `herdr::keys_argv(pane, "Enter"),` |
| `src/harness/hermes.rs:53` | R17 | `herdr::wait_working_argv(&p.agent, "10000"),` |
| `src/harness/hermes.rs:54` | R4 | `herdr::read_argv(pane),` |
| `src/harness/hermes.rs:60` | R18 | `herdr::exec(r, &c[0])?;` |
| `src/harness/hermes.rs:62` | R7 | `herdr::exec(r, &c[1])?;` |
| `src/harness/hermes.rs:63` | R6 | `herdr::exec(r, &c[2])?;` |
| `src/harness/hermes.rs:64` | R17 | `if herdr::exec(r, &c[3])?.code == 0 {` |
| `src/harness/hermes.rs:67` | R4 | `let screen = herdr::exec(r, &c[4])?;` |
| `src/harness/mod.rs:39` | — comment/doc | `/// herdr argv that launches the harness in 'pane'.` |
| `src/harness/mod.rs:41` | — comment/doc | `/// herdr commands that settle the harness, deliver the kick and confirm it landed.` |
| `src/herdr.rs:1` | — comment/doc | `//! The one herdr adapter (herdr ≥ 0.9.0, docs/setup.md): every 'herdr' argv is built here` |
| `src/herdr.rs:9` | R0 | `pub const BIN: &str = "herdr";` |
| `src/herdr.rs:11` | R0 | `/// Run a herdr argv built here ('*_argv'); the caller reads the exit code.` |
| `src/herdr.rs:13` | R0 | `r.run("herdr", a)` |
| `src/herdr.rs:16` | R0 | `/// The full command line of a herdr argv, as a dry-run plan shows it.` |
| `src/herdr.rs:18` | R0 | `std::iter::once("herdr".to_string()).chain(a).collect()` |
| `src/herdr.rs:23` | R0 | `runner::ok(r, "herdr", a)` |
| `src/herdr.rs:28` | R0 | `runner::json(r, "herdr", a)` |
| `src/herdr.rs:31` | R1 | `/// 'herdr status': client and server versions ('herdsman doctor').` |
| `src/herdr.rs:36` | R2/R3 | `/// 'herdr agent list' or 'workspace list' ('what'), as JSON.` |
| `src/herdr.rs:41` | R2/R3 | `/// Live herdr agents ('herdr agent list').` |
| `src/herdr.rs:49` | R2/R3 | `/// 'herdr agent list''s stdout when it ran OK (serve's watcher hashes it).` |
| `src/herdr.rs:66` | R5 | `/// The pane's last 'lines' lines (herdr's whole 'recent' window when shorter), with their` |
| `src/herdr.rs:96` | R9 | `// No '--': herdr 0.9 reads the 2nd positional as literal text (even '-x'), and a '--' the` |
| `src/herdr.rs:105` | R9 | `bail!("herdr {} {}: {}", a[0], a[1], o.stderr.trim());` |
| `src/herdr.rs:121` | R12 | `/// herdr's '{"error": {"code": <code>}}' answer (on stderr or stdout), e.g. 'timeout'.` |
| `src/herdr.rs:136` | R14 | `/// 'herdr pane get''s pane, when a harness (any detected agent) runs in it.` |
| `src/herdr.rs:142` | R15 | `/// 'herdr pane run <pane> <words…>': type a line into the pane's shell.` |
| `src/herdr.rs:147` | R16 | `/// 'herdr agent start' for the harnesses herdr knows ('--kind'), 'args' going to the harn` |
| `src/herdr.rs:155` | R17 | `/// 'herdr agent wait' until the kick shows as working or blocked.` |
| `src/herdr.rs:210` | R22 | `/// Remove the lane's worktree through herdr (by its workspace); false when herdr would no` |
| `src/host.rs:3` | R2 | `pub use crate::herdr::agents;` |
| `src/host.rs:19` | R0 | `const SPARE: &[&str] = &[crate::herdr::BIN, "herdsman", "launchd"];` |
| `src/hosts.rs:2` | — comment/doc | `//! as a Runner: herdr gets '--machine', every other tool runs over 'ssh'. No '[hosts]' = ` |
| `src/hosts.rs:35` | R25 | `if prog == crate::herdr::BIN {` |
| `src/hosts.rs:241` | — text/path | `ex("~/.herdr/worktrees")` |
| `src/lanes.rs:1` | — comment/doc | `//! The page's Lanes tab (docs/serve.md): herdr agents joined with the log, grouped, plus ` |
| `src/lanes.rs:23` | — comment/doc | `/// herdr's live state (idle \| working \| done \| blocked \| unknown), or 'dead' \| 'park` |
| `src/lanes.rs:30` | — comment/doc | `/// #157: the '[hosts]' host it runs on, when fleet.toml has hosts, and its herdr machine ` |
| `src/lanes.rs:35` | — comment/doc | `/// Join 'herdr agent list' rows with the log. 'ws' maps workspace id -> label.` |
| `src/lanes.rs:142` | — comment/doc | `/// Section order on the page; any other herdr state (unknown) counts as idle.` |
| `src/lib.rs:25` | — use/mod | `pub mod herdr;` |
| `src/lib.rs:90` | — text/path | `worktrees: env("FLEET_WORKTREES", home().join(".herdr/worktrees")),` |
| `src/lib.rs:356` | R7 | `let Some(pane) = pane.filter(\|_\| herdr::prompt(r, agent, &msg).is_ok()) else {` |
| `src/lib.rs:368` | R6 | `herdr::keys(r, pane, "Enter")?;` |
| `src/lib.rs:369` | R10 | `if !herdr::landed(r, agent, pane, &msg) {` |
| `src/overlap.rs:104` | R9 | `if crate::herdr::reply(&on, sp.and_then(\|e\| e.str("harness")) == Some("dsh"), lane, pane` |
| `src/runner.rs:1` | — comment/doc | `//! The one seam for external tools (herdr, gh, git). Tests swap in a fake.` |
| `src/serve.rs:8` | — use/mod | `use crate::{devices, gh, herdr, lanes, queue, Ctx};` |
| `src/serve.rs:194` | — comment/doc | `/// The watcher's last look: the log's (size, mtime), the 'herdr agent list' read (secs, h` |
| `src/serve.rs:198` | R2 | `herdr: (u64, u64),` |
| `src/serve.rs:503` | — comment/doc | `/// The lane's wait reason while herdr shows it idle, done or unknown.` |
| `src/serve.rs:641` | R2 | `if now >= w.herdr.0 + 2 {` |
| `src/serve.rs:642` | R2 | `let out = herdr::agent_list(self.r);` |
| `src/serve.rs:643` | R2 | `changed \|= hash(&out) != w.herdr.1;` |
| `src/serve.rs:644` | R2 | `w.herdr = (now, hash(&out));` |
| `src/serve.rs:645` | R2 | `lock(&self.cache).insert("herdr agent list".into(), (now, out));` |
| `src/serve.rs:694` | R4 | `lock(&self.cache).retain(\|k, _\| !k.starts_with("herdr pane read"));` |
| `src/serve.rs:891` | R2 | `.context("herdr agent list failed")?;` |
| `src/serve.rs:915` | R4 | `let screen = \|\| herdr::read(&on, &pane);` |
| `src/serve.rs:935` | R6 | `herdr::keys(&on, &pane, choice)?;` |
| `src/serve.rs:946` | — comment/doc | `/// 'herdr agent prompt' (dsh: 'pane send-text') + Enter; logs a 'prompt' event without th` |
| `src/serve.rs:962` | R9 | `herdr::reply(&l.on(self.r), l.harness == "dsh", agent, pane, text)?;` |
| `src/serve.rs:1003` | — comment/doc | `// #218: a lane with no pane (herdr no longer lists it): its stored transcript, no live bl` |
| `src/serve.rs:1009` | R5 | `let screen = herdr::recent(&l.on(&self.cached(ctx.now, 5)), l.pane.as_deref()?, "200").ok(` |
| `src/serve.rs:1096` | R2 | `let n = crate::check::leftovers(ctx, &cfg, self.r, &herdr::agents(self.r)?, true)?.len();` |
| `src/serve.rs:1284` | R4 | `.and_then(\|p\| herdr::read(&l.on(&self.cached(ctx.now, 2)), p).ok())` |
| `src/serve.rs:1335` | — comment/doc | `/// 'herdr agent\|workspace list' JSON, read at most every 10 s (None when herdr cannot be` |
| `src/serve.rs:1336` | R2/R3 | `fn herdr_json(&self, now: u64, what: &str) -> Option<serde_json::Value> {` |
| `src/serve.rs:1337` | R2/R3 | `herdr::list(&self.cached(now, 10), what).ok()` |
| `src/serve.rs:1346` | — comment/doc | `// The state of an open lane herdr shows no pane for: 'dead', or 'parked' (#83).` |
| `src/serve.rs:1398` | — comment/doc | `// #205: a lane herdr still lists opens its Lanes sheet (serve.js '.ql'); #218: a gone one` |
| `src/serve.rs:1445` | — comment/doc | `// The current lane name links only while herdr lists it; otherwise it stays plain text.` |
| `src/serve.rs:1453` | R2 | `card.push("<span class=r>dead: the log has this lane open, herdr has no such agent</span>"` |
| `src/serve.rs:1629` | — comment/doc | `/// herdr's agents joined with the log (None when herdr cannot be read).` |
| `src/serve.rs:1631` | R2 | `let list = self.herdr_json(now, "agent")?;` |
| `src/serve.rs:1634` | R3 | `let wsl = self.herdr_json(now, "workspace").unwrap_or_default();` |
| `src/serve.rs:1716` | R2 | `return "<p class=r>herdr agent list failed</p>".into();` |
| `src/serve.rs:1753` | R4 | `.and_then(\|p\| herdr::read(&l.on(&self.cached(ctx.now, 30)), p).ok());` |
| `src/serve.rs:1784` | R2 | `card.push("<span class=r>⚠ log/pane mismatch: the log has this lane open, herdr has no pan` |
| `src/serve.rs:1859` | R2 | `let top = host::top(r, &herdr::agents(r).unwrap_or_default()).join(" · ");` |
| `src/serve.rs:1889` | R2 | `let left = cfg.zip(herdr::agents(r).ok()).map_or(vec![], \|(c, a)\| {` |
| `src/serve.rs:1916` | R0 | `let svc = \|s: &&crate::timer::Row\| ["board", "tick", herdr::BIN].contains(&s.0.as_str())` |
| `src/spawn.rs:7` | — use/mod | `use crate::herdr;` |
| `src/spawn.rs:124` | R15 | `let mut c = herdr::run_argv(pane, &["export", &lane, &format!("HERDSMAN_PORT={port}")]);` |
| `src/spawn.rs:321` | — comment/doc | `/// The herdr argv that creates the lane's worktree.` |
| `src/spawn.rs:323` | R19 | `herdr::worktree_create_argv(local, &p.branch, &p.base, &p.worktree.to_string_lossy(), &p.a` |
| `src/spawn.rs:337` | R19 | `cmds.push(herdr::line(create_argv(&local, p)));` |
| `src/spawn.rs:338` | R15 | `cmds.push(herdr::line(p.lane_cmd("<pane>")));` |
| `src/spawn.rs:339` | R16 | `cmds.push(herdr::line(p.start_cmd(ctx, "<pane>")));` |
| `src/spawn.rs:340` | R6/R7/R17/R18 | `cmds.extend(p.kick_cmds("<pane>").into_iter().map(herdr::line));` |
| `src/spawn.rs:496` | R2 | `let agents = host::agents(r).context("cannot list herdr agents for the duplicate guard")?;` |
| `src/spawn.rs:587` | R11 | `herdr::close(r, pane)?;` |
| `src/spawn.rs:613` | R20 | `let open = herdr::worktree_open_argv(&local, &wt, &p.agent);` |
| `src/spawn.rs:614` | R19/R20 | `let created = herdr::json(r, &if reopen { open } else { create_argv(&local, &p) })?;` |
| `src/spawn.rs:649` | R19 | `.context("herdr: no workspace_id")?;` |
| `src/spawn.rs:650` | R21 | `herdr::workspace_rename(r, ws, &p.agent)?; // #194: an open workspace keeps its branch lab` |
| `src/spawn.rs:653` | R23/R24 | `let mut pane = herdr::free_pane(r, ws, &p.worktree, None)?;` |
| `src/spawn.rs:655` | R15 | `herdr::ok(r, &home(p.lane_cmd(&pane)))?;` |
| `src/spawn.rs:656` | R16 | `let mut start = herdr::exec(r, &home(p.start_cmd(ctx, &pane)))?;` |
| `src/spawn.rs:658` | R12 | `if herdr::failed_with(&start, "agent_pane_busy") {` |
| `src/spawn.rs:660` | R11 | `let closed = lane && !agents.iter().any(\|a\| a["pane_id"] == pane.as_str()) && herdr::clo` |
| `src/spawn.rs:662` | R23/R24 | `pane = herdr::free_pane(r, ws, &p.worktree, Some(&pane))?;` |
| `src/spawn.rs:663` | R15 | `herdr::ok(r, &home(p.lane_cmd(&pane)))?;` |
| `src/spawn.rs:664` | R16 | `start = herdr::exec(r, &home(p.start_cmd(ctx, &pane)))?;` |
| `src/spawn.rs:667` | R12 | `let taken = herdr::failed_with(&start, "agent_name_taken");` |
| `src/spawn.rs:671` | — comment/doc | `// Under load herdr can time out while the harness still comes up: keep it as the lane.` |
| `src/spawn.rs:673` | R12 | `let why = if !herdr::failed_with(&start, "timeout") {` |
| `src/spawn.rs:674` | R16 | `Some(format!("herdr agent start failed: {}", start.stderr.trim()))` |
| `src/spawn.rs:675` | R14 | `} else if herdr::harness_in(r, &pane).is_none() {` |
| `src/spawn.rs:677` | R16 | `"herdr agent start timed out and no harness came up: {}",` |
| `src/spawn.rs:681` | R13 | `herdr::ok(r, &herdr::rename_argv(&pane, &p.agent))` |
| `src/spawn.rs:686` | R11 | `let _ = herdr::close(r, &pane);` |
| `src/spawn.rs:720` | R11 | `let _ = herdr::close(r, old);` |
| `src/spawn.rs:795` | R14 | `let info = herdr::harness_in(r, pane).with_context(\|\| format!("no harness running in pan` |
| `src/spawn.rs:796` | R13 | `herdr::ok(r, &herdr::rename_argv(pane, &p.agent))?;` |
| `src/timer.rs:5` | — use/mod | `use crate::{drive, herdr, host, Ctx};` |
| `src/timer.rs:16` | R26 | `const HANDOFF: &str = "restart herdr to finish the update: 'herdr update --handoff'";` |
| `src/timer.rs:66` | — comment/doc | `/// The jobs' PATH: the dirs of the harness binaries, 'gh', 'herdr' and herdsman, then the` |
| `src/timer.rs:72` | R0 | `.chain(["gh", herdr::BIN])` |
| `src/timer.rs:151` | — comment/doc | `/// A refusal exits 0; a real error (config, herdr) logs 'tick-failed', alerted at most ho` |
| `src/timer.rs:160` | R1 | `herdr::status(r).map_err(\|e\| anyhow::anyhow!("herdr: {e:#}"))?;` |
| `src/timer.rs:268` | — comment/doc | `/// herdr's (client, server) versions, and whether the server is older ('herdr status').` |
| `src/timer.rs:269` | R1 | `pub fn herdr_versions(r: &dyn Runner) -> Option<(String, String, bool)> {` |
| `src/timer.rs:270` | R1 | `let status = herdr::status(r).ok()?;` |
| `src/timer.rs:301` | — comment/doc | `/// The Host tab's Services rows from the event log, 'herdr', the host and the stamps; 'bo` |
| `src/timer.rs:329` | R1 | `let v = herdr_versions(r);` |
| `src/timer.rs:333` | R0 | `rows.push(row(herdr::BIN, t, d, [herdr::BIN, HANDOFF][usize::from(t == "a")]));` |
| `src/waiting.rs:276` | — comment/doc | `/// One row per lane herdr does not show 'working': the latest lane's reason ('s.lane == w` |
| `src/waiting.rs:392` | — comment/doc | `// #214: a reviewer whose lane exited (no herdr agent) without a verdict gets a new review` |
