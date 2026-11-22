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
| when | `hosts::register`, at the start of the tick's check step and of `drover hosts refresh`: one `machine.links`, then `machine.add` for each enabled host it does not list. Twice is once |
| a taken name | shed answers `INVALID_PARAMS` for another target under the same name. drover logs `host-refused` once, doctor's row is `✗ host box: shed's machine box has another target → shed machine rm box`, and no lane goes there |
| removal | never by drover. A host removed from fleet.toml is sent nothing; the machine stays in shed (cooee and the owner may use it). A cloud host's end is `vm.destroy`, from D6 |
| the event log | a spawn's `machine` stays the target, as today, so old events read the same. The shed name is found through `[hosts]` |
| a session | at this host's shed a lane on box is `box/<name>`. `crate::shed` adds and strips the prefix; `session::Session` keeps its bare name and its host |
| a cloud host | until D6 a `[hosts]` entry like any other. From D6 its VM is the machine, of the same name, and `register` skips it |

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

A machine with no entry in `links` reads as `Down` since now, with "not registered" as its reason.

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
| `account` on a fleet and on a role | new: a name or `"auto"`; unset is `"auto"` (#1576) | D2 |
| `[proxy]` | deleted | D2 |
| `[providers.deepseek]`'s key file | deleted; `warn_below` stays | D8 |
| a cloud recipe override in drover's config folder | moves to `<config>/vms/` of shed, once | D6 |

Each PR that deletes or renames a key the owner may have set carries a `## Rollout` section with the exact lines.

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
# D0. One tag for every shed crate: two tags are two sources, and `deny.toml` refuses the second shed-proto.
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
