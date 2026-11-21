# Shapes

## Shards (slice 1)

```sh
# scripts/test-shards.sh <1-4>: shard n's cargo flags. The whole rule, #1547's for an unlisted target:
for t in unit $(ls crates/*/tests/*.rs crates/*/tests/*/main.rs | sed -E 's|.*/tests/||; s|/main\.rs$||; s|\.rs$||' | sort -u); do
  [ $(($(cksum <<<"$t" | cut -d' ' -f1) % 4 + 1)) = "$1" ] || continue
  if [ "$t" = unit ]; then printf -- '--lib --bins '; else printf -- '--test %s ' "$t"; fi
done
```

`unit` is every crate's lib and bin tests, as today. Deleted: `scripts/test-shards.txt`, the `check` form, the
`rust` job's "test shards" step. The `test-shard` job's call does not change.

## CI gates (slice 2)

```text
ci/gates.sh run <job>        every ci/gates/<job>/*.sh in name order, each in a ::group::; all run;
                             a failure is an ::error:: and a job-summary row; exit 1 if any hard gate failed
ci/gates.sh touched <job>    stdin: changed paths; exit 0 when one starts with a `# paths:` entry of a gate

ci/gates/rust/
  10-fmt.sh  12-clippy.sh  14-lint-fixtures.sh  30-perf.sh  32-screen-hub.sh  34-soak.sh  40-demo-leak.sh
  44-deps.sh  46-layers.sh  48-no-ts-rs.sh  50-deny.sh  52-ts-types.sh  54-html.sh  56-timing.sh  58-parity.sh
  60-cap.sh  62-arch-check.sh  64-lesson-check.sh  66-assets-budget.sh
ci/gates/ui-webkit/
  first-paint.sh  phone-timings.sh  events-pages.sh  thread-tap.sh  parity.sh  parity-red.soft.sh  url-links.sh
  page-drafts.sh  keyboard.sh  threads.sh  issue-sheet.sh  herd.sh
```

A gate file:

```sh
#!/usr/bin/env bash
# gate: <today's step name, with its issue numbers>
# paths: <space-separated path prefixes whose change runs the job>      (ui-webkit only; optional)
set -euo pipefail
<today's `run:` lines, unchanged>
```

- Line 2 is the name the log group, the error and the summary row show.
- `*.soft.sh` fails as a `::warning::`, never red (today's `continue-on-error: true`).
- A gate that ran only on some events tests them itself: `34-soak.sh` exits 0 unless `EVENT` is `push` and `REF` is
  staging's. The workflow passes `EVENT`, `REF`, `BASE` and `BODY` (the PR body) in the step's `env`.
- `40-demo-leak.sh` is today's demo export and leak gate: the second reads the first's output.
- The numbers in `rust` only put the cheap gates first in the log. Two files may share a number.

`ci.yml` after, per job:

```yaml
  rust:
    steps:
      # checkout (fetch-depth 0), toolchain, rust-cache, node, install-action (just, cargo-deny): as today
      - name: build
        run: cargo build --release --locked
      - name: gates (ci/gates/rust, one log group each; every failure named)
        run: ci/gates.sh run rust
        env: { EVENT: "${{ github.event_name }}", REF: "${{ github.ref }}", BASE: "${{ github.event.pull_request.base.sha }}", BODY: "${{ github.event.pull_request.body }}" }

  changes:
    steps:
      # checkout; the staging and unknown-base cases answer webkit=true: as today
      - run: |
          git diff --name-only "$BASE...$HEAD" | tee changed.txt
          webkit=false
          if grep -qE '^(web/|assets/|crates/server/|ci/gates)' changed.txt || ci/gates.sh touched ui-webkit < changed.txt; then webkit=true; fi
          echo "webkit=$webkit" | tee -a "$GITHUB_OUTPUT"

  ui-webkit:
    steps:
      # checkout, toolchain, rust-cache, node, playwright webkit: as today
      - name: gates (ci/gates/ui-webkit)
        run: ci/gates.sh run ui-webkit
```

`test-shard`, `test`, `actionlint`, `web`, `ui` and `ui-flaky` keep their steps. The workflow's name, its triggers
and every job id and name are unchanged.

## The limits table (slice 3)

```rust
// crates/core/src/cli.rs: `config show`
/// The built-in defaults as docs/config/limits.md's Markdown table; reads no fleet.toml
#[arg(long)]
md: bool,

// crates/core/src/config.rs
/// `drover config show --md`: every `[limits]` key, its default and its doc line, as the docs table.
pub fn show_md(out: &mut dyn std::io::Write) -> Result<()>;
```

```just
gen: types
    # … today's four lines
    cargo run -q --locked -p drover -- config show --md > docs/config/limits.md
    find fleet.example.d -type f | LC_ALL=C sort | xargs cat > fleet.example.toml
```

The file is today's `## Limits (#844)` section: its heading, its paragraph, then
`| key | default | what it sets |` with one row per `LIMIT_DOCS` entry (`deny_commands` shows
`assets/deny_commands.toml`, as the hand-written row does).

## The example (slice 3)

```text
fleet.example.d/
  00-head.toml              the file's opening comment
  10-serve.toml             [serve]
  20-fleet-api.toml         [fleet.api] and its roles
  30-fleet-drover.toml      [fleet.drover] and its roles
  40-fleet-web/
    00-table.toml           [fleet.web] and its required keys (repo, local, integration)
    <key>.toml              one optional key each: its line or lines, with the comment, as today
  45-fleet-web-tables.toml  [[fleet.web.serial]] … [fleet.web.role.verify]: the tables under it
  50-proxy.toml  55-providers.toml
  60-limits.toml            [limits] and one comment: every key and default is in docs/config/limits.md
  70-notify.toml  80-setup.toml  90-runtime.toml
```

Path order is byte order: `00-table.toml` sorts before every key file, and the key files sort among themselves by
name. The joined file parses as TOML exactly as today's does (`crates/cli/tests/setup_neutral.rs` reads it).

```rust
// crates/core/src/propose.rs, in `view`: the help text of one field
doc: docs.get(key).copied().or_else(|| limit_doc(&path)).unwrap_or_default().into(),

/// `LIMIT_DOCS`' line for a `limits.<key>` path.
fn limit_doc(path: &str) -> Option<&'static str>;
```

## Docs (slice 4)

```text
docs/serve.md    frozen index: the title, the page's first paragraph, "Each section is a file in docs/serve/."
docs/serve/      page.md  report.md  alerts.md  webhooks.md  health.md  api-v1.md  screens.md  export.md
docs/lanes/      design.md  flow.md  spawn.md  github.md  hook.md  drive.md  asks.md  check.md
docs/hosts/      add.md  roots.md  tags.md  placement.md  hook-rules.md  remote-asks.md  off.md  monitoring.md
                 health.md  update.md  cloud.md  bootstrap.md  acp.md  runners.md  limits.md
docs/config/     serial.md  fast-first.md  rename.md  merge-label.md  hotspots.md  baseline.md  in-flight.md
                 done-checks.md  hygiene.md  public.md  limits.md (generated)
```

One file per `##` section, its `###` parts with it; each file's first line is the section's heading as `#`. Short
neighbours share a file: `tags.md` holds Tags and Needs, `update.md` Auto-update and "When the main host is down",
`serial.md` the page's opening with Joining and Slots.

Pointed at the new files:

| what | today | after |
|---|---|---|
| `README.md` | `docs/serve.md#serve-health-898` | the health file |
| `docs/ci.md` | `config.md#fast-first-landing-350` | the fast-first file |
| `docs/setup.md` | `lanes.md#drive` | the drive file |
| `fleet.example.toml` | `docs/config.md#hygiene-1101` | the hygiene file |
| `crates/cli/tests/webhook.rs`, `crates/cli/tests/setup_neutral.rs` | read `docs/serve.md` | read the section's file |
| `crates/cli/tests/limits.rs` | compares the table in `docs/config.md` | the comparison is deleted |
| `.drover/docs.md`, first line | `docs/*.md`: one page per area | adds: four of them are a folder of sections |

## Screens (slice 5)

```rust
// crates/server/src/serve/screens/mod.rs

/// A screen request: its path, its query, and whether a paired device asks.
pub(super) struct Req<'a> {
    pub path: &'a str,
    pub q: &'a Q,
    pub paired: bool,
}

/// An action as posted: its name, the block and screen it came from, its form values, and the loaded config.
pub(super) struct Call<'a> {
    pub ctx: &'a Ctx,
    pub cfg: &'a Config,
    pub name: &'a str,
    pub block: &'a str,
    pub from: &'a Value,
    pub values: &'a Value,
}

impl Call<'_> {
    pub fn s(&self, key: &str) -> String;      // a value as text; "" when absent
    pub fn fleet(&self) -> String;
    pub fn issue(&self) -> Result<u64>;        // "no issue" when absent
}

/// What an action answers.
pub(super) enum Reply {
    /// A toast line; `close` closes the sheet the action came from. The router adds the cards it removed.
    Said { toast: String, close: bool },
    /// The whole answer as built: a replaced screen, a dry run's sheet.
    Whole(Value),
}

impl Reply {
    pub fn said(toast: String) -> Reply;       // `Said` that leaves the sheet open
}

/// One area of the screens: its routes and its actions, in its own file.
pub(super) struct Area {
    /// Its screen at the request's path; `None` when the path is not its own, or the screen has no data.
    pub screen: fn(&Server, &Ctx, &Req) -> Option<Value>,
    /// Its answer to the action; `None` when the name is not its own.
    pub action: fn(&Server, &Call) -> Option<Result<Reply>>,
    /// Its actions the owner may run though `cooee-plugin.toml` lists none of them.
    pub owner: &'static [&'static str],
    /// Its full screens carry the snapshot's age (every area but threads).
    pub age: bool,
}

const AREAS: &[Area] = &[
    queue::AREA, lanes::AREA, events::AREA, cards::AREA, fleets::AREA, hosts::AREA, scene::AREA, threads::AREA,
];
```

Where today's items go (`crates/server/src/serve/screens/*.rs`):

| file | from | routes | actions |
|---|---|---|---|
| `mod.rs` (the router) | `build_screen`, `screens`, `action`'s frame, `plugin`, `form_op`, `paired_screen` | none | none |
| `queue.rs` | `queue_screen`, `fleet_card`, `row`, `done_screen`, `backlog`, `stats`, `counts`, the hot sheet | queue, queue/done, backlog, hot, issue, verdict | filter, drive, drive-all, force, pause, top, drop, move, queue, limit, unpark |
| `issue.rs` (queue's) | `issue_sheet`, `check_rows`, `history`, `rounds`, `verdict_sheet` | | |
| `lanes.rs` | `lanes_screen`, `lane_row`, `lane_figure`, `lane_sheet`, `dry_run` | lanes, lanes/<agent> | stop, kill, clean, close, send |
| `events.rs` | `events_screen`, `groups`, `when`, `kind_of` | events | filter (from the events screen) |
| `cards.rs` | the ask, needs-human, run and health cards, `ask_sheet`, `health_sheet` | cards, ask, drive-health | answer, needs-human, run-card, config-card, later |
| `later.rs` (cards') | today's `later.rs` | | |
| `fleets.rs` | today's `screens_fleets.rs`, `workflow` | fleets, notify, workflow | save-fleets, remove-fleet |
| `hosts.rs` | today's `screens_hosts.rs` | hosts, proxy-token | `HOST_ACTIONS` |
| `scene.rs` | `scene_screen`, `scene_repo`, `band`; `herd_tests.rs` as its tests | scene | none |
| `threads.rs` | `THREAD_ACTIONS` and their arm | threads, threads/<id> (built by `serve/chat.rs`) | the thread actions, thread-signin, secret-card |
| `blocks.rs` | `out`, `line`, `Res`, `ticking`, `iso`, `cut`, `section`, `rows_list`, `said`, `figure`, the age row | | |
| `places.rs` | `permitted`, `places`, `page_places`, `screen_files` | | |
| `ops.rs` | `ops` and its tree walk | | |
| `local.rs`, `pages.rs` | today's `screens_local.rs`, `screens_pages.rs` | | |

`local.rs`'s permission check reads `AREAS`' `owner` lists where it reads `THREAD_ACTIONS` and `HOST_ACTIONS` today.
The `filter` action is two arms: events' matches when the call came from the events screen, queue's otherwise.

## The page script (slice 6)

No code. `arch-check --write-baseline` runs once for the new `web/src` rule.
