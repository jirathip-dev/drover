# Architecture and state

herdsman has no daemon and no database. Every command reads the config, folds the event
log into the current state, acts, and appends what it did.

## Event log

`~/.local/state/herdsman/events.jsonl` (`FLEET_STATE`) is append-only, one JSON object per
line, fsynced on every write. A `spawn` event, shortened:

```json
{"ts":1761800000,"fleet":"app","issue":12,"type":"spawn","role":"impl","agent":"app-impl-12","pane":"w1:p2"}
```

`ts` is seconds since the epoch, `type` is the event kind, and the other fields are the
event's data. Issue state (`queued → impl → review(n) → fix(n) → ready → merged`, plus
`blocked` and `needs-human`) and the queue are folds over this log. A writer that died
mid-line leaves a torn tail; the next append terminates it first so the two records never
merge. A merged issue stays `merged`; `spawn` and `adopt` on it are refused with `MERGED`.

## Paths

| Path | What |
|---|---|
| `~/.config/herdsman/fleet.toml` (`FLEET_CONFIG`) | config: fleets, roles, limits, notifiers |
| `~/.local/state/herdsman/events.jsonl` (`FLEET_STATE`) | the event log |
| `~/.local/state/herdsman/cache/` | cached issue specs and PR/CI reads |
| `~/.herdr/worktrees/` (`FLEET_WORKTREES`) | lane worktrees, `<repo>/issue-<n>` |
| `~/.config/herdsman/claude/<profile>/` | `settings.json` (PreToolUse hook), `role.md`, `fleet.md` |
| `~/.hermes/profiles/<profile>/config.yaml` (`FLEET_HERMES_PROFILES`) | sync rewrites its `hooks:` key and drops `agent.reasoning_effort` (fleet.toml `effort` owns it) |
| `~/.config/herdsman/dsh/<profile>.patch.yml` | DSH patch layer for `harness = "dsh"` lanes ([dsh.md](dsh.md)) |
| `roles/`, `hooks/` | role prompts, the tail of `help --agent` (`usage-tail.md`) and hook settings, compiled into the binary |

A path variable that is set but empty counts as unset.

## Rename from `fleet`

herdsman was called `fleet` (#27). Only `herdsman migrate` moves `~/.config/fleet` and
`~/.local/state/fleet` to the paths above, both or neither; `HERDSMAN_HOME` overrides
`$HOME` for it. Until then other commands read the old dirs in place and print a one-line
hint on stderr (`hook` stays silent). Lanes export `HERDSMAN_LANE`; `FLEET_LANE` still arms
the hook for one release, and `HERDSMAN_LANE` wins when both are set.

## Module map

Three crates (#264): `crates/core` (every module below but `serve`), `crates/server` (`serve`, over core only) and
`crates/cli` (the `herdsman` binary: core plus server, and the integration tests). `cargo install --path crates/cli`.
Core never depends on server or cli: `herdsman serve` reaches it through the `SERVE` hook `main.rs` sets. CI checks
both rules: no server or cli crate in core's dependency tree, and `std::process::Command` in core only in `runner`
(the `Runner` implementation), `events` (notifiers) and `webpush` (curl).
The split is a pure move by what compiles today, so core still holds I/O modules (`drive`, `spawn`, `harness`,
`devices`, `webpush`): their cycles (`gate` → `spawn`/`drive`/`check`, `config` → `harness`/`devices`,
`events` → `webpush`, `serve` → `drive` → `cli`) run through them. #265–#269 move them out step by step.

Every module under `crates/*/src/`, one job each. External tools go through one seam: `runner` (any program), `herdr` (every
herdr argv) and `gh` (cached GitHub reads).

| Module | Job |
|---|---|
| `cli` | clap CLI, `help --agent`, dispatch (`crates/cli`: `main.rs` sets the `SERVE` hook) |
| `lib` | `Ctx` (paths, clock); `ask`, `answer`, `asks` |
| `config` | fleet.toml: load, validate, in-place edits that keep every other byte |
| `events` | the append-only log, the state fold, notifiers |
| `pipeline` | a fleet's stages and where each outcome goes ([pipeline.md](pipeline.md)) |
| `queue` | `queue`, `next`, `board` |
| `drive`, `timer` | one pass of the loop; the jobs that run it (`setup`, `timer`, `tick`) and the Services rows |
| `spawn`, `harness/*` | `spawn`, `adopt`, `profiles sync`; one file per harness (claude, hermes, dsh) |
| `gate` | `verdict` and `merge` |
| `check` | `check` alerts, `clean`, `stop`, `kill`, `close`, the worktree budget |
| `waiting` | what an idle lane waits on (`waiting`) |
| `host`, `hosts` | host-load admission and live agents; multi-host placement ([hosts.md](hosts.md)) |
| `access`, `usage` | a lane's account and MCP servers; account and provider usage (`accounts`) |
| `hook` | the lanes' pre-tool hook |
| `overlap` | conflict preview between open branches, `cap-check` |
| `serve`, `lanes`, `devices`, `webpush` | the phone page, its Lanes tab, paired devices, web push |
| `doctor`, `protection` | read-only host and branch-protection checks |
| `runner`, `herdr`, `gh` | the seams to external programs |

## API types

Every JSON shape the page reads or posts has one definition, in Rust: `crates/server/src/api.rs` (`/herd.json`, the
lane chat, `/live`'s `ev`, API v1's tabs (#269: the needs cards, Lanes, [chat]), the `/send`, `/answer`, `/act` and `/setup` bodies) and the core rows the
board shows (`Event` for events and asks, `IssueState`, `Waiting`, `HostSeen`, `HostRow`). `ts-rs`, behind the server's
`ts` feature (never in the release build; CI checks `cargo tree -e normal`), turns them into `assets/types/api.d.ts`:
`just types` writes it (`cargo test -p herdsman-server --features ts export_bindings`). The types follow serde's JSON:
u64 is `number`, `Option<T>` is `field?: T | null`, `rename` and `flatten` as serde does them. CI regenerates the file
and fails on any diff, then `serde_json_fits_the_types` type-checks the demo export's real payloads and each type's
default with `tsc --noEmit --strict`, and `just types-lint` fails a hand-written copy of a generated type in the board's
TS: it imports them from `assets/types/api`.

## Code rules

- **Format:** rustfmt with `max_width = 120` (`rustfmt.toml`); CI runs `cargo fmt --check`. The formatter decides
  layout, not the lane: no golf, no joined or packed lines to fit the cap. Run `cargo fmt` before committing.
- **Lints:** clippy `pedantic` as warnings: `[workspace.lints]` in `Cargo.toml` for core and server (#264), `#![warn(clippy::pedantic)]` in the binary's `main.rs`; CI's `-D warnings` makes it an error. Other
  exceptions sit inline at the narrowest scope as `#[allow(clippy::<lint>)] // reason`. The crate-level allows, all
  style noise for a CLI:
  - `must_use_candidate`: return values here are read or deliberately dropped; `#[must_use]` everywhere is noise.
  - `missing_errors_doc`: nearly every fn returns `anyhow::Result`, and the error is the message it prints.
  - `missing_panics_doc`: the few `unwrap`s are on invariants, not inputs.
  - `too_many_lines`: dispatch and page renderers are long matches; splitting them only moves lines.
  - `format_push_string`: the HTML page is built with `+= &format!(..)`; one extra allocation per row is nothing.
  - `format_collect`: `.map(|x| format!(..)).collect()` reads better than a `fold` with `write!`.
  - `many_single_char_names`: short names in small math and parsing closures (dates, hex, crypto).
- **Size cap:** at most `line_cap` (in `ci/cap.toml`) formatted lines of non-test Rust under `src/` and `crates/*/src/` (tests
  live in `crates/cli/tests/`), counted after `cargo fmt`. CI runs `herdsman cap-check --config ci/cap.toml`; a fleet can set its own
  `line_cap` under `[fleet.<name>]`. Raising the cap needs the owner's OK in an issue.

History: one file per change in `docs/cap-history/<date>-<issue>.md`, so no two PRs edit the same line.
