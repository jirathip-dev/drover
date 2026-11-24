# main-host-only: the data shapes after the cut (#1691)

Every shape here is today's with fields removed. The slice that removes each is in brackets (`adr.md`, "Slices").

## The main host's entry

```rust
/// `[hosts.<name>]`: the main host's limits. At most one entry is left after `Config::parse`.
pub struct HostCfg {
    pub tags: Vec<String>,
    pub root: Option<String>,
    pub roots: Vec<String>,
    pub deny_commands: Option<Vec<Deny>>,
    pub deny_commands_add: Vec<Deny>,
    pub allow_commands: Option<Vec<String>>,
    pub max_working: Option<usize>,
    pub start_max: Option<usize>,
    pub start_release: Option<StartRelease>,
    pub max_simulators: Option<usize>,
    pub heavy_max: Option<usize>,
    pub heavy_kind_max: BTreeMap<String, usize>,
    pub heavy_wait_minutes: Option<u64>,
}
```

Removed: `cloud`, `size`, `budget_usd_month`, `max_hours_day`, `idle_stop_minutes`, `github_token` [2];
`runtime` [3]; `enabled` [6]; `machine` [6, as a field the code reads; see below].

`Config.hosts` stays a map, so `[hosts.<name>]` is written as today and the name stays what the Host tab, the
history rows and the `spawn` event show. With no entry the main host is `local`, as today.

```rust
impl Config {
    /// The main host: its `[hosts]` name and keys, else `local` with none.
    pub fn main(&self) -> (&str, Option<&HostCfg>);
}
```

`main` replaces the seven places that search the map for the entry without `machine` [9].

## Old keys

| Old key | At load | Told to the owner |
|---|---|---|
| `[hosts.<n>]` with `machine` | the entry is dropped; it never reads as a second main host [6] | one `Config::warnings` row: doctor's `!` row, the board's config line, `check`'s `config-warning` once |
| a second `[hosts.<n>]` without `machine` | the first by name is the main host, the rest are dropped [6] | the same row |
| `cloud`, `size`, `budget_usd_month`, `max_hours_day`, `idle_stop_minutes`, `github_token`, `runtime`, `enabled` on the main entry | ignored | nothing |
| `[proxy]` [3], `[fleet.<n>.soak]` [5], `[fleet.<n>.role.<r>] host` [6], `[ci_runners]` [10] | ignored | nothing |
| the seventeen `[limits]` keys below | ignored | nothing |
| `"runners"` in `[orchestrator] allow` | still accepted; it allows nothing [10] | nothing |

`Config`, `HostCfg`, `Fleet`, `Role` and `Limits` take unknown keys, so "ignored" needs no code. Two things
do. The dropped entry and its warning row are the only code the cut adds: about 10 lines in `config.rs`, on the
retired-key path `[accounts]` and `[telegram]` already use, with one test beside
`crates/cli/tests/retired_keys.rs`'s three. And `orchestrator.allow` is checked against a closed list at load,
so slice 10 leaves the `runners` name in that list: a fleet.toml that still allows it must keep loading.

## Where a lane goes

```rust
pub struct Placed {
    pub name: String,                               // the main host's name
    pub root: PathBuf,                              // the workspace root picked
    pub local: PathBuf,                             // the repo checkout
    pub refusal: Option<(&'static str, String)>,    // LOGIN_NEEDED | DISK_LOW | MEM_LOW | HOST_BUSY | NO_HOST
    pub why: Option<String>,
}
```

Removed: `machine`, `home` [9]; `rank` [6].

## What the last look saw

```rust
/// `<state dir>/hosts.json`, one entry: the main host.
pub struct Seen {
    pub tags: Vec<String>,
    pub probed: u64,
    pub seen: u64,
    pub up: bool,                                   // its own shed answered the last look
    pub load: Option<f64>,
    pub mem: Option<f64>,
    pub disk: Option<f64>,
    pub pressure: Option<u8>,
    pub swap: Option<f64>,
    pub top: Vec<String>,
    pub mem_total: Option<f64>,
    pub disk_total: Option<f64>,
    pub roots: Vec<crate::roots::Root>,
}
```

Removed: `drover` [1]; `busy`, `github` [2]; `health` [5]; `home` [9]. `Seen` is the API's `HostSeen`,
so `assets/types/api.d.ts` is regenerated in those slices. `Row`, the history file and the charts do not change.

## Sessions

```rust
pub struct Session { name, labels, cwd, acp, pid, cpu_pct, rss_mb, life, note }   // no host, no machine [9]

/// This host's sessions, or why its shed did not answer.
pub struct Sessions(pub Result<Vec<Session>, String>);                             // was one answer per host [9]
```

## The log

A new `spawn` event carries `host` (the main host's name) and `root`, and no `machine` [9]. Old events keep
theirs. One predicate reads it:

```rust
impl Event {
    /// False for an old event of a host drover no longer reaches (it names a `machine`).
    pub fn here(&self) -> bool;
}
```

It is used where today's code compares `e.str("machine")` with a host: the login hold, a lane's folders, the
secrets files left, the watcher's worktree list, `clean`. So an old `signed-out` event of another host never
holds the main host, and `clean` never looks on this disk for a folder that was on another one.

The kinds below are no longer written. They stay in `events/passive.rs` and `events/push.rs`: those lists say
how an old event in the log is read.

| Slice | Kinds no longer written |
|---|---|
| 1 | `host-sync`, `host-behind`, `host-sync-failed`, `self-update-swapping` |
| 2 | `cloud-start`, `cloud-up`, `cloud-stop`, `cloud-abort`, `cloud-repin`, `cloud-snapshot` |
| 3 | `proxy-ok`, `proxy-key-revoked`, `proxy-token-rejected`, `proxy-token-expiring` |
| 4 | `outbox`, `outbox-stale`, `asks-imported` |
| 5 | `soak` |
| 6 | `host-off`, `lane-moved`, `lanes-moved`, `stall-move` |
| 7 | `host-offline`, `host-online`, `host-key-changed`, `hosts-skipped` |
| 10 | `runners-*` |

## The link's frame

```rust
pub struct Worktree { path, head, dirty, verdict }     // no `lines` [4]
pub struct Watch { roots, worktrees, beat_secs, poll_millis, sample_secs }   // unchanged
```

`Line` goes with the outbox [4]. `Frame`, `Report`, `Seen` and the view's tables do not change: the main host's
own watcher still feeds them.

## `[limits]` keys removed

| Slice | Keys |
|---|---|
| 1 | `self_update_lag_minutes`, `self_update_swap_secs` |
| 2 | `cloud_idle_stop_minutes`, `overflow_after_minutes` |
| 3 | `proxy_connect_secs`, `proxy_retry_secs` |
| 4 | `outbox_stale_minutes`, `import_max_age_hours` |
| 7 | `ssh_connect_secs`, `ssh_quick_secs`, `ssh_call_secs`, `ssh_sessions`, `ssh_retry_secs`, `host_offline_minutes`, `host_down_secs`, `serve_remote_secs` |
| 8 | `sweep_remote` |

Kept: every `link_*` key (the main host's own watcher), `shed_call_secs`, `host_wait_minutes`, `start_max`,
`max_parallel_spawns`, the disk, memory and load floors. `docs/config/limits.md` and `fleet.example.toml` are
regenerated by `just gen` in each of those slices.

## Refusal codes that end

`HOST_UPDATING` [1], `NO_PROXY_TOKEN` [3], `DESIGN_MAIN_ONLY` and `MAIN_HOST` [6], `HOST_OFFLINE` [7]. The
first three and the last each have one file under `roles/usage/30-refusals/`, deleted in the same slice.
`NO_HOST` stays, with one meaning: the main host lacks a tag the lane needs.

## The flow table's host fact

```
today   Host { Up, Loaded, Draining, Offline }
after   Host { Up, Loaded }                       [7]
```

The `host-offline` row of `crates/core/flow/issue/build.toml` goes with its timeout key `host_offline_minutes`,
and `draining` leaves the rows that list it. `docs/flow.md` is regenerated.
