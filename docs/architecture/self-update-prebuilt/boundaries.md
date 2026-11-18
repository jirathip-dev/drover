# Module boundaries

```
cli ──► update ──► release ──► runner (gh, and the shell line a host runs)
           │
           ├──► hosts    members: [hosts], cached tags and home, On, real_exe, link_line, acp_profiles
           ├──► cloud    state: is a cloud host running
           ├──► timer    the serve restart, heal, the staging serve job
           └──► events   the marks and alerts

cloud::bootstrap, cloud::start ──► release    this build's sha, staged and swapped on one host
hosts::survey ──► release::builds             a version read; never an install
hosts::at, spawn's ACP guard ──► hosts::updating    the log only, no ssh
check ──► events                              host-sync-failed, as today
```

## Who may call what

| Module | May call | May not |
|---|---|---|
| `core::release` (new) | `runner` | `events`, `config::Config`, `hosts`, `update`: it takes a runner and plain values, and returns types |
| `update` | `release`, `hosts`, `cloud` (state only), `timer`, `events` | call `land` (no CI read); be called by `hosts`, `spawn` or `check` |
| `hosts` | `release::builds` in the survey | `release::stage`, `update`: a survey never installs |
| `cloud` | `release` for bootstrap and start | `update` |
| `spawn` | `hosts::acp_guard`, `hosts::updating` | `release`, `update` |
| the release workflow | nothing in the repo but the build and its one script | `ci.yml`'s jobs; a secret outside the macOS job |

Three rules the reviewer can check in the diff:

1. Only `update`'s swap and `cloud`'s bootstrap and start ever run `swap_line`. Nothing else renames a binary.
2. Before a swap, the only path any code writes on a host is `drover.new` (and its `.part`).
3. `update` asks GitHub one thing: `release::find`. It no longer reads CI's check runs; a release that exists is
   the proof CI was green.

## What is deleted

| Where | What goes |
|---|---|
| `crates/core/src/update.rs` | `stage` (worktree and cargo build), `gate`'s signing step, the `sha` marker file, the `land::suite` read |
| `crates/core/src/hosts.rs` | `sync`, `install_line`, `SYNC_STATUS`, `SYNC_START`, the `host-sync-skipped` event |
| `crates/core/src/cloud.rs` | bootstrap's `cargo install` of drover (shed's stays) |
| `crates/core/src/gate.rs` | the build-and-install line `drover merge` prints for this repo |
| scripts/install-gate.sh | the file |
| `docs/setup.md`, `docs/hosts.md` | the "Self-update" and "Auto-update" sections, each replaced by a link to the new page; the manual install becomes a download of the release asset |

## Tests

The entry point is `drover self-update` with the fake runner and the fake ssh hosts the host tests already use;
the clock is `ctx.now`.

- Four hosts, three staged, one unreachable: at 29 minutes no swap line has run on any host; at 31 minutes three
  have, in order with the Mini last; one `host-behind`; a placement for the fourth answers `HOST_UPDATING`; the log
  holds no refusal with `ACP_UNGUARDED`.
- No release: no build or download call was made; past the wait limit one `self-update-waiting`; a third pass adds
  none; a release appearing installs.
- A wrong checksum, and a `drover.new` whose version names another sha: the live binary's bytes are unchanged,
  `drover.new` is gone, one `self-update-failed`.
- A pass killed between a boat's swap and the Mini's: the next pass swaps only what is left.
- An old process whose guard finds a boat on the new build: `HOST_UPDATING`, and the host's health row is not `✗`.
- `step` alone, as a table test: one row per arrow of the rollout statechart.
