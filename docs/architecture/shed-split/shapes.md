# Shapes: hosts as machines, the `Host` type, the keys and the pins (#1567)

For the impl lanes and the reviewer. shed's own types are in shed's ADRs and are not repeated here.

## `[hosts]` become shed machines

```toml
[hosts.mini]                       # no `machine`: this host. Never registered; its calls stay in drover's runner.
[hosts.box]
machine = "user@box.example"       # unchanged key and value: now also the target of shed's machine "box"
```

| rule | |
|---|---|
| name | the `[hosts]` key is the shed machine's name. shed takes letters, digits, `-` and `_`; `Config::load` refuses any other key that has a `machine` |
| target | the `machine` value, as written: `[user@]host` or an ssh alias |
| today | #1616, on staging: every drive pass runs `shed machine add <machine> <machine>` for each enabled host shed does not list, so the name is the `machine` value. The first of D2b and D1 to land names it by the key. Where key and value are equal nothing changes; where they differ, that PR's rollout has the owner's `shed machine rm <old name>` line |
| when | `hosts::register`, at the start of every drive pass, as today, and of `drover hosts refresh`: one `machine.links`, then `machine.add` for each enabled host it does not list. Twice is once |
| a taken name | shed answers `INVALID_PARAMS` for another target under the same name. drover logs `host-refused` once, doctor's row is `✗ host box: shed's machine box has another target → shed machine rm box`, and no lane goes there |
| removal | never by drover. A host removed from fleet.toml is sent nothing; the machine stays in shed (cooee and the owner may use it). A rented VM's machine is added and removed by shed's helper (#1644) |
| the event log | a spawn's `machine` stays the target, as today, so old events read the same. The shed name is found through `[hosts]` |
| a session | at this host's shed a lane on box is `box/<name>`. `crate::shed` adds and strips the prefix; `session::Session` keeps its bare name and its host |
| a cloud host | until #1644 a `[hosts]` entry like any other, and its link alone says whether it is up: `cloud::state` decides spend and nothing else. #1623's fix (`e24a53998`) did the first half of that before D1: listed `up` by shed, it is up. From #1644 the helper adds its machine and `register` skips it |

## `crate::shed`

```rust
pub struct Shed { /* shed_client::Shed on this host's socket */ }

pub struct Host<'a> {
    shed: &'a Shed,
    machine: Option<String>,          // none: this host
    r: &'a dyn Runner,                // this host's commands; a test's fake
}

impl Shed {
    pub fn local(ctx: &Ctx) -> Result<Shed>;                       // SHED_VERSION under the client's floor
    pub fn here<'a>(&'a self, r: &'a dyn Runner) -> Host<'a>;
    pub fn host<'a>(&'a self, cfg: &Config, name: &str, r: &'a dyn Runner) -> Result<Host<'a>>;
    pub fn of<'a>(&'a self, cfg: &Config, spawn: Option<&Event>, r: &'a dyn Runner) -> Host<'a>;   // was `On::of`
    pub fn links(&self) -> Result<BTreeMap<String, Link>>;
}

impl Runner for Host<'_> { /* this host: `r`; a machine: `exec` with the caller's limit, else `host_call_secs` */ }

impl Host<'_> {
    pub fn machine(&self) -> Option<String>;                       // for `RunParams`, `ExecParams`, `FileParams`
    pub fn exists(&self, p: &Path) -> bool;
    pub fn existing<'p>(&self, paths: &[&'p str]) -> BTreeSet<&'p str>;   // one `exec`, the paths on stdin
    pub fn read(&self, p: &Path) -> Option<String>;                // file.read
    pub fn write(&self, p: &Path, text: &str) -> Result<()>;       // file.write; folders made by one `exec mkdir -p`
    pub fn write_private(&self, p: &Path, text: &str) -> Result<()>;      // mode 0o600; deleted by D3
    pub fn remove(&self, p: &Path) -> Result<()>;
    pub fn sample(&self, top: u32, last: Option<Swap>) -> Result<host::Host>;   // machine.stats {top}: `now` only
    pub fn procs(&self, session: &str) -> Result<Tree>;            // terminal.procs
    pub fn report(&self, w: &Watch) -> Result<Frame>;              // D11
}

// shed/release.rs (D4): drover's own binary, the one thing drover asks shed to install. The tool, its
// releases and its path on a host are fixed inside; no function here takes a tool name or a path.
impl Shed {
    pub fn release(&self, build: &str) -> Result<Release>;         // install.find, once, on the main host
}
impl Host<'_> {
    pub fn builds(&self) -> Result<Install>;                       // install.ls: live, staged, previous
    pub fn stage(&self, rel: &Release) -> Result<Install>;         // install.stage, with drover's gate and identifier
    pub fn swap(&self, build: &str) -> Result<Install>;            // install.swap, with drover's links on PATH
    pub fn back(&self) -> Result<Install>;                         // install.back
}

pub struct Swap { pub at: u64, pub pages: u64 }                    // one `now`: `Sample::at`, `swap_out_pages`
pub struct Tree { pub at_ms: u64, pub procs: Vec<Proc> }           // one terminal.procs answer, and when it came

/// Percent of the host's CPUs the tree used between two reads of it.
pub fn busy(a: &Tree, b: &Tree, cpus: u64) -> f64;                 // pure

pub enum Reach {
    Up,
    Draining { since: u64 },                 // up, and leaving (#1644): no new lane, every call
    Down { since: u64, why: String },        // the link is not up: the pass sends this host nothing
    Offline { since: u64, why: String },     // down longer than `host_offline_minutes`
}
pub fn reach(links: &BTreeMap<String, Link>, host: &str, now: u64, l: &Limits) -> Reach;   // pure
```

`reach` is the one place that reads shed's link. At D1 its input is `machine.links`:

| the link's `state` | `Reach` |
|---|---|
| `up` | `Up` |
| `reconnecting` | `Down` since `down_since`: shed is dialing, a call would not arrive |
| `down` | `Down` since `down_since`, with the link's `error` |
| either, with `down_since` older than `host_offline_minutes` | `Offline` |
| no entry | `Down` since now, "not registered" |

`machine.links` says nothing of a drain, so D1 never returns `Draining`. The step that moves the input to
`machine.ls` does (D13, or #1644's lane-host slice if it lands first), and changes no caller:

| the machine's `flow` | its link's `flow` | `Reach` |
|---|---|---|
| `up` | `up`, `slow` | `Up`; a `slow` host is placed on last |
| `up` | `hung`, `reconnecting` | `Down` since the link's `since`: shed keeps the machine `up` while it dials, and answers a call `NO_LINK` |
| `draining` | `up`, `slow` | `Draining` since the machine's `since` |
| `draining` | `hung`, `reconnecting` | `Down` |
| `added` | any | `Down`, "never up yet" |
| `offline` | any | `Down`, then `Offline` by the age of its `since` |
| `key_changed`, `removed` | any | `Offline` at once, with the reason and who did it: one alert each |

## #1649's host fact: two inputs, read in drive's pass

#1649's `host` is up, loaded, draining or offline (its ADR's facts file, in the flow-table folder on branch `issue-1649`). `Reach` is one of
its two inputs and stays as above: the link, four values, none of them `loaded`. The other is the host's own
sample. Drive's pass is the only code that reads either from shed, once per host:

```rust
let reach = reach(&links, name, now, l);               // the link: one `machine.links` a pass, no network
let sample = host.sample(3, last).ok();                // the host's own: `machine.stats` `now`, taken there by its shed;
                                                       // asked only of a host that is `Up` or `Draining`; admission reads the same one
view.host_seen(name, &reach, sample.as_ref());         // the host's row (#1654's `HostSeen`): a missing sample keeps the last, with its `at`
```

| #1649's value | read from | never from |
|---|---|---|
| `offline` | `Reach::Offline`; `Reach::Down` by #1649's own rule (after `2 × fast_secs`, unless load at either end explains it) | a slow or failed `exec`, a `machine.stats` that did not come |
| `loaded` | the row's last sample over its limits: `load_per_cpu` and `mem_level` (X1), the numbers admission reads. Which limits is #1649's rule | `Reach`, a link's `slow` flow, how long a call took, the main host's load |
| `draining` | `Reach::Draining`; beside it the orders drover gives itself (`hosts off`, a release swap), which #1654's `hosts::drain` holds | |
| `up` | `Reach::Up`, the last sample under its limits, no order | |

- **One sample, two readers.** The pass's one `machine.stats` per host serves `host::admit` and the row. Room
  for one more lane stays admission's answer (`HOST_BUSY`); `loaded` is #1649's reading of the same numbers.
- **The main host's row** is `shed.here(r)`'s sample, its own. It changes no other host's value.
- **Read in the pass only.** `facts::host` takes the row and no `Shed`, so a kick, `waiting`, serve and the API
  process cannot call shed for the fact: they read what the last pass stored.
- **What D1 moves.** #1654 puts a sample in each report frame (`host::sample` on the reporting host) and lets
  link intake write the row. From D1 the pass writes the row's link, sample and drain; the frame's `sample` and
  `host::sample` go. `sys::loadavg` stays for `runner::hung` on this host (#1652): it starts no process. The
  row's "heard" clock reads the link's `down_since`. The four values, the bound and every row of #1649's table
  stay.

## A rate is two reads

shed gives counts and a life's average. drover takes every rate as a difference, in `crate::shed`:

| rate | the two reads | kept between them |
|---|---|---|
| swap-out, pages per 2 s | this pass's `now` and the last pass's, for the same host: pages over seconds, times 2. A count that went down (a reboot) gives no rate that pass | `Swap` and the last five rates, in `hosts.json` beside the host's last sample |
| a lane's CPU share | two `terminal.procs` of its terminal, `cpu_sample_secs` apart, in the same tick's chore or the same `top` run | nothing |

`busy` sums, over the processes in the second read, the `cpu_ms` each gained since the first. A process is the
same one when its pid matches and its start (`at` less `elapsed_s`) does within two seconds; one that started between the reads counts
whole; one that ended counts nothing. The sum over the gap and the CPUs is the share. `cpu_ms` is X1's
addition to `Proc`: CPU time over its life, in milliseconds. lane-hog asks only for the lanes of a host over its
load limit, and still wants 3 ticks running; `top` asks for every lane it lists.

## What admission reads, and from where

`host::admit` and its `[limits]` do not change. Its input, `host::Host`, is filled from shed:

| drover reads | today | after D1 |
|---|---|---|
| memory pressure level | `sysctl kern.memorystatus_vm_pressure_level` | `Sample::mem_level` (X1); none on Linux, as today |
| swap-out, pages per 2 s | `vm_stat`, against a count kept in `swapcount*` files | the difference of `swap_out_pages` between this pass's `now` and the last pass's (above); never `history` |
| its median of 5 | the `swapouts` file | the last five rates, kept in `hosts.json`; both files go |
| load per CPU, without drover's own | `sysctl`, `ps`, minus drover's processes | `now.load_per_cpu`, minus the `top_cpu` rows whose command is drover |
| free and total memory | `vm_stat`, `hw.memsize`, `/proc/meminfo` | `mem_free_mb`, `mem_total_mb` |
| the 3 busiest processes not drover's | `ps` | `top_cpu` rows with no `terminal`: named in the refusal, their `cpu_pct` never used as a rate |
| free and total disk | `df -k /` | `disk_free_mb`, `disk_total_mb`: a reading only, for `DISK_LOW` and the chart (open question 2; the other answer is `roots::free` for both). A workspace root on another volume stays `df` by `exec`, in `roots.rs` |
| a lane's CPU share | `ps`'s `pcpu`, joined to `lsof` by folder | `busy` over two `terminal.procs` reads (above); never `cpu_pct` |
| a lane's memory | the same join | `terminal.procs`: `rss_mb` summed over its tree |
| the top memory users and their lane | `top`, `lsof` | `top_mem` rows, each with its `terminal` |
| `os`, `arch`, `$HOME` | a probe script | `Link::os`, `arch`, `home` |
| `xcode`, `docker`, `webkit` | the same script | that script without its first three lines, by `exec`, hourly as today |
| the host's shed | `ssh … shed connect`, `hello` | `Link::hello` |

`hosts::Seen` (`hosts.json`) keeps drover's own facts: tags, health rows, roots, the gh token row, `busy`, the
host's drover build, the last sample for the board, and with it the swap count and the five rates.

## Who owns what on a host

The owner's decision of 2026-11-23: a shed machine is a place to run agents, and a host's setup is shed-cloud's
recipe (shed #337), on pool VMs and, from a run card, on the Mini and the Air. No step of this ADR moves anything
across these rows. A CI runner is not a host of this table: the Mini's native runners, the Docker slots and the
runner VMs, with their image, toolchains and sccache, are `jirathip-dev/runners`'.

| on a host | whose | drover's part |
|---|---|---|
| the link, the processes, their CPU and memory | shed | reads them through `crate::shed` (D1) |
| shed's own binary and state | shed | nothing |
| drover's own binary | shed's `install.*` moves it, on drover's order (D4) | which build, when, the swap order, the check after |
| toolchains and tools: git, gh, just, Rust, node, sccache | the recipe | installs none. The tags probe reads what is there (`xcode`, `docker`, `webkit`) for placement; bootstrap's installs go with #1644 |
| the shared caches: sccache's folder, pnpm's store | the recipe: where they are and how big; the tools trim themselves | `spawn` sets a lane's cache variables (#1641), which ride the start's `env`; `reclaim`, `clean` and doctor report a cache's size and delete none |
| the disk floor | the recipe: it trims caches to get back over it, never a worktree or a lane's folder | `DISK_LOW` refuses a start under `min_free_gb` and names both remedies; it frees lane folders only |
| a lane's worktree and its build folders (`target/`, `node_modules/`, …) | drover: lane state | the tick's trim, `reclaim`, `clean`, `DISK_BUDGET`; unchanged, through a `Host` from D1 |
| the clones, the profiles, the sign-in panes | drover | bootstrap's own steps, as today |

## fleet.toml

| key | change | step |
|---|---|---|
| `[limits] ssh_connect_secs`, `ssh_quick_secs`, `ssh_sessions`, `ssh_retry_secs` | deleted | D1 |
| `[limits] ssh_call_secs` | renamed `host_call_secs`: a call to a machine is cut after this long | D1 |
| `[limits] link_beat_secs`, `link_stale_secs`, `link_retry_max_secs` | deleted | D11 |
| `[limits] link_poll_millis` | kept: the least time between two reports of one host | D11 |
| `[limits] link_sample_secs` | kept: a report this often with no state change | D11 |
| `[limits] link_down_minutes`, `host_offline_minutes` | kept: both now read the link's `down_since` | D1 |
| `[limits] cpu_sample_secs` | new, default 5: the gap between the two process reads a CPU share is taken from | D1 |
| `account` on a fleet, on a role and on `[fleet.<n>.role.orchestrator]` | new: a name or `"auto"`; unset is `"auto"` (#1576) | D2a |
| `[proxy]`, `[limits] proxy_connect_secs` | deleted | D2b |
| `[providers.deepseek]`'s key file | deleted; `warn_below` stays | D8 |
| the cloud keys, a cloud recipe override | #1644's to decide | #1644 |
| `[ci_runners]`, the recipe's `runner_*` lines | deleted with `runners.rs`, once `jirathip-dev/runners` covers them | #1644, as amended |

Each PR that deletes or renames a key the owner may have set carries a `## Rollout` section with the exact lines.

## The account key (D2a, D2b)

```toml
[fleet.web]
account = "work"                    # every role of the fleet, unless the role says otherwise
[fleet.web.role.rev]
account = "auto"                    # what no key at all means
[fleet.web.role.orchestrator]
account = "personal"                # its threads: `orchestrator.rs` `start()` reads the same key
```

```rust
pub enum Account { Auto, Named(String) }               // config.rs; `"auto"` is the one reserved name

impl Config {
    pub fn account(&self, fleet: &str, role: &str) -> Account;   // the role's, else the fleet's, else Auto
}
pub struct Acp { /* cmd, mcp, meta, awake */ pub account: Option<Account> }   // term.rs; none: no `--account` word
```

| rule | |
|---|---|
| who has one | an ACP start: a lane's (`spawn`) and a thread's (`orchestrator.rs` `start()`). A terminal lane (dsh) has none |
| where, D2a | the main host only. On another host `Acp::account` is none and drover's proxy signs the lane in, as today |
| where, D2b | every host, for an agent whose record has `[proxy]`. Another agent on another host: none when no key is set (that host's login), `ACCOUNT_LOCAL_ONLY` when one is (open question 1) |
| `"auto"` | shed picks among the provider's signed-in named accounts and moves a lane at a limit. `default`, the CLI's own login, is used only when named |
| the gate | doctor, from the pin PR on: one row per agent that a role or an orchestrator starts over ACP. `✓ account claude: work, personal` names its signed-in named accounts (`ready` or `limited`, never `default`); `✗ account codex: no signed-in named account → shed account add codex <name>`. D2a's build is installed when every row is `✓` |
| a wrong name | `Config::load` checks only the form (shed's letters, digits, `-`, `_`). Whether the account exists is shed's answer at the start, `NO_ACCOUNT`: accounts change without a config edit |
| retired keys | `accounts`, `fallback` and `[accounts]` stay retired, with the warning they have today; `account` leaves that list |
| the floor | `shed::MIN_PROTOCOL` is 5 from D2a |
| the event log | a spawn and a thread start record `account` as the terminal reports it, so the board can show which one a lane is on |

## The link's report (D11)

```rust
pub struct Watch {
    pub roots: Vec<String>,
    pub worktrees: Vec<String>,
    pub acked: Vec<(String, u64)>,     // new: (worktree, sent) of outbox lines the main host has run
}
```

`drover hosts report` writes each `acked` pair to the worktree's acked mark before it reads the outbox, which is
what the watcher did with an ack line on its stdin. `beat_secs`, `poll_millis` and `sample_secs` leave `Watch`;
`Body::Beat`, `Frame::boot` and `Frame::n` leave the frame.

The outbox itself does not change at any step: `.drover/outbox.jsonl` and its acked mark in the worktree, one
line a verb, run once on the main host as the lane its worktree's spawn names. It is the only way a lane on
another host reaches the main host (#1646), so which verbs write it is #1646's list, and the pull is this report.

## The pins

```toml
# The first pin PR, before D2a: today's line with a tag in place of `rev = "9b88eb5…"` (protocol 2 to 5).
shed-proto = { git = "https://github.com/jirathip-dev/shed", tag = "client-v5.1" }
# D0, in place of the line above. One tag for every shed crate: two tags are two sources, and `deny.toml`
# refuses the second shed-proto.
shed-client = { git = "https://github.com/jirathip-dev/shed", tag = "client-v<P>.<n>" }
# D9
shed-catalog = { git = "https://github.com/jirathip-dev/shed", tag = "client-v<P>.<n>" }
# D3, at the first tag that carries the crate (shed's H1 adds it to the tag job)
shed-secrets = { git = "https://github.com/jirathip-dev/shed", tag = "client-v<P>.<n>" }
# D10, at the commit that has F1
farrier-service = { git = "https://github.com/jirathip-dev/farrier", rev = "<40 hex>" }
```

`shed-proto` leaves `[workspace.dependencies]`: drover names the wire types as `shed_client::proto`. `deny.toml`:

```toml
allow-git = [
    "https://github.com/jirathip-dev/shed",
    "https://github.com/jirathip-dev/blocks",
    "https://github.com/jirathip-dev/swag",
    "https://github.com/jirathip-dev/farrier",
]
```
