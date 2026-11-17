# Shapes

## The list (slice 1)

```rust
const DAYS: u64 = 14;
const HOT: usize = 5;         // the floor
const HOT_IN: usize = 20;     // hot: touched by 1 in 20 of the counted merges
const SWEEP: usize = 40;      // a merge touching more files is a sweep: not counted

/// What the window held, for the report's header.
pub struct Window {
    pub merges: usize,        // counted
    pub sweeps: usize,        // skipped
    pub bar: usize,           // max(HOT, merges / HOT_IN)
    pub shared: usize,        // files over the bar that arch.toml lists `shared`
}

pub fn find(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<(Window, Vec<Hot>)>;
```

`Hot` and `hotspots-<fleet>.json` keep their shape; `prs` now counts narrow merges only. `find` groups the log's
files per commit first, drops a commit with more than `SWEEP` files, then counts. `touched.retain` also drops what
`arch::decided` matches.

```rust
/// tick: store each fleet's list when it is a day old; file its issue when the last one is 7 days old.
pub fn daily(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()>;
fn store(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<(Window, Vec<Hot>)>;   // find + write the json
fn file(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, body: &str) -> Result<()>;
```

`run` (the `drover hotspots` command) is `store` then `file`, as today. Two stamp files beside the state:
`hotspots-<fleet>.at` (the issue, kept) and `hotspots-<fleet>.day` (the store, new). `drive`'s `sync` is unchanged:
it logs `hot-files` when the stored list less the decided files differs from the last one.

## Shared files (slice 2)

```rust
struct Arch {
    // … today's keys
    /// Files many PRs touch by nature: off the hot list, no serial group; each under a `[[size]]` rule.
    #[serde(default)]
    shared: Vec<String>,
}

/// The globs an arch.toml lists as frozen, generated or shared: decided, so never on the hot list. Was `converted`.
pub fn decided(text: &str) -> Result<Vec<String>>;
```

`check` adds one violation: a `shared` glob no `[[size]]` rule's `paths` can match.
`drover arch-check --write-baseline` runs once in the PR. The baseline then lists 4 files over 2,000 lines in `src`,
4 test files over 1,500, and each function over 150 at today's size.

## One generator (slice 3)

```just
# every generated file (.drover/arch.toml `generate`); arch-check runs it and fails on a difference
gen: types
    npm --prefix web ci --no-audit --no-fund
    npm --prefix web run build
    cargo run -q --locked -p drover -- diagram > docs/workflow.md
    BLESS=1 cargo test -q --locked -p drover --test brief_golden
```

Deleted from `.github/workflows/ci.yml`: the `web` job's `git diff --exit-code -- dist` lines (its `npm run build`
stays, as the bundle check) and the `rust` job's `workflow diagram` step. Follow-up D appends one line:

```just
    cargo run -q --locked -p drover -- config show --md > docs/limits.md
```

## Phone alerts by flag (slice 4)

```rust
pub struct Event {
    // … seq, ts, fleet, issue, kind, passive
    /// #1147: reaches a phone whose notifier sets no `on`. Kinds pushed before the flag are in `events::push::LEGACY`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub push: bool,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

impl Event {
    pub fn push(mut self) -> Event { self.push = true; self }
    pub fn is_push(&self) -> bool { self.push || push::LEGACY.contains(&self.kind.as_str()) }
}

impl Notify {
    pub fn wants(&self, e: &Event) -> bool;   // was `wants(&self, kind: &str)`
}
```

```rust
// crates/core/src/events/push.rs: frozen. The 44 kinds of today's WEBPUSH_ON; it only shrinks.
pub const LEGACY: &[&str] = &["slo-api", "stuck", "config-invalid", /* … */];
```

`check::fire` keeps its signature for its 32 calls and builds the event; it delegates to `fire_event`, which takes
a built `Event` and reads `is_push()` for the per-pass push budget. A new pushable alert calls `fire_event`. The TS
type gains `push?: boolean` through `just gen`.

## The help tail (slice 5)

```text
roles/usage/
  10-decisions.md   20-states.md   40-lessons.md   50-exit-codes.md   60-github.md   70-guard.md   80-events.md
  30-refusals/
    00-head.md            the `## Refusal codes` heading
    BAD_SHA.md  BAD_VALUE.md  BASE_MISMATCH.md  …   one `- CODE: text` line each, alphabetical
```

```rust
// crates/core/build.rs
let mut tail = String::new();
for p in sorted(walk("roles/usage")) { tail += &read(p); }      // path order; one blank line between sections
write(out_dir.join("usage-tail.md"), tail);
println!("cargo:rerun-if-changed=roles/usage");

// usage_doc
s + include_str!(concat!(env!("OUT_DIR"), "/usage-tail.md"))
```

A bullet that names several codes today is split, one file per code. The tests that read `help --agent` (m3, repro,
access, readme) read the joined text and stay as they are.

## CI per area (follow-up A)

```text
.github/workflows/
  rust.yml        job `rust`: checkout, toolchain, node, tools, build, test, then one step: ci/gates.sh
  web.yml         job `web`
  ui.yml          jobs `ui`, `ui-flaky`
  ui-webkit.yml   jobs `changes`, `ui-webkit`
  lint.yml        job `actionlint`
ci/gates.sh       runs ci/gates/*.sh in name order, each in a `::group::`; runs all, then exits 1 naming each failure
ci/gates/
  10-fmt.sh  12-clippy.sh  20-no-temp-dirs.sh  30-perf.sh  32-soak.sh  40-demo-export.sh  42-leak.sh  44-deps.sh
  46-layers.sh  48-no-ts-rs.sh  50-deny.sh  52-ts-types.sh  54-html.sh  56-timing.sh  58-parity.sh  60-cap.sh
  62-arch-check.sh  64-lesson-check.sh  66-assets-budget.sh
```

Each gate is a bash script whose first comment line is today's step name. `rust.yml` passes `EVENT` and `BODY`
(the PR body) in the step's env for the arch-check gate. Job ids and names do not change, so each fleet's
`required_checks` and branch protection match as before. `ci.yml` is deleted in the same PR.

## Docs per area (follow-up B)

```text
docs/serve.md     an index of at most 15 lines, frozen: one line on the page, "each section is a file in docs/serve/"
docs/serve/       page.md  themes.md  report.md  alerts.md  webhooks.md  health.md  api-v1.md  screens.md  export.md
docs/lanes/       design.md  flow.md  spawn.md  github.md  hook.md  drive.md  asks.md  check.md
docs/hosts/       one file per `##` section of today's docs/hosts.md (17)
```

Text moves unchanged. Links and rule lines that name the old file keep working: it still exists. The docs check
already reads `docs/**/*.md`.
