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
| a cloud host | until #1644 a `[hosts]` entry like any other, and its link alone says whether it is up: `cloud::state` decides spend and nothing else (#1623). From #1644 the helper adds its machine and `register` skips it |

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
    pub fn sample(&self, top: u32) -> Result<host::Host>;          // machine.stats {top, since: now - 600}
    pub fn procs(&self, session: &str) -> Result<Vec<Proc>>;       // terminal.procs
    pub fn report(&self, w: &Watch) -> Result<Frame>;              // D11
}

pub enum Reach {
    Up,
    Down { since: u64, why: String },        // the link is not up: the pass sends this host nothing
    Offline { since: u64, why: String },     // down longer than `host_offline_minutes`
}
pub fn reach(links: &BTreeMap<String, Link>, host: &str, now: u64, l: &Limits) -> Reach;   // pure
```

A machine with no entry in `links` reads as `Down` since now, with "not registered" as its reason. `reach` is
the one place that reads shed's link: D13 changes its input to `machine.ls` and its `flow`, and no caller.

## What admission reads, and from where

`host::admit` and its `[limits]` do not change. Its input, `host::Host`, is filled from shed:

| drover reads | today | after D1 |
|---|---|---|
| memory pressure level | `sysctl kern.memorystatus_vm_pressure_level` | `Sample::mem_level` (X1); none on Linux, as today |
| swap-out, pages per 2 s | `vm_stat`, against a count kept in `swapcount*` files | the difference of `swap_out_pages` between two samples of `history` |
| its median of 5 | the `swapouts` file | the last five differences in `history`; both files go |
| load per CPU, without drover's own | `sysctl`, `ps`, minus drover's processes | `now.load_per_cpu`, minus the `top_cpu` rows whose command is drover |
| free and total memory | `vm_stat`, `hw.memsize`, `/proc/meminfo` | `mem_free_mb`, `mem_total_mb` |
| the 3 busiest processes not drover's | `ps` | `top_cpu` rows with no `terminal` |
| free and total disk | `df -k /` | `disk_free_mb`, `disk_total_mb`; a workspace root on another volume stays `df` by `exec`, in `roots.rs` |
| a lane's CPU share and memory | `ps` joined to `lsof` by folder | `terminal.procs`: the sum over its tree |
| the top memory users and their lane | `top`, `lsof` | `top_mem` rows, each with its `terminal` |
| `os`, `arch`, `$HOME` | a probe script | `Link::os`, `arch`, `home` |
| `xcode`, `docker`, `webkit` | the same script | that script without its first three lines, by `exec`, hourly as today |
| the host's shed | `ssh … shed connect`, `hello` | `Link::hello` |

`hosts::Seen` (`hosts.json`) keeps drover's own facts: tags, health rows, roots, the gh token row, `busy`, the
host's drover build, and the last sample for the board.

## fleet.toml

| key | change | step |
|---|---|---|
| `[limits] ssh_connect_secs`, `ssh_quick_secs`, `ssh_sessions`, `ssh_retry_secs` | deleted | D1 |
| `[limits] ssh_call_secs` | renamed `host_call_secs`: a call to a machine is cut after this long | D1 |
| `[limits] link_beat_secs`, `link_stale_secs`, `link_retry_max_secs` | deleted | D11 |
| `[limits] link_poll_millis` | kept: the least time between two reports of one host | D11 |
| `[limits] link_sample_secs` | kept: a report this often with no state change | D11 |
| `[limits] link_down_minutes`, `host_offline_minutes` | kept: both now read the link's `down_since` | D1 |
| `account` on a fleet, on a role and on `[fleet.<n>.role.orchestrator]` | new: a name or `"auto"`; unset is `"auto"` (#1576) | D2a |
| `[proxy]`, `[limits] proxy_connect_secs` | deleted | D2b |
| `[providers.deepseek]`'s key file | deleted; `warn_below` stays | D8 |
| the cloud and runner keys, a cloud recipe override | #1644's to decide | #1644 |

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

## The pins

```toml
# The first pin PR, before D2a: today's line with a tag in place of `rev = "9b88eb5…"` (protocol 2 to 5).
shed-proto = { git = "https://github.com/jirathip-dev/shed", tag = "client-v5.0" }
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
