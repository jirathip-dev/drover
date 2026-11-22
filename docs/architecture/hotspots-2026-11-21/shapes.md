# Shapes

This PR's slices: 1, 3, 4 and 6. The screens (slice 5, #1556) are in `screens.md`; the CI gates (slice 2) are #1587's.

## Shards (slice 1)

```text
scripts/test-shards.d/
  2026-11-22.txt      today's scripts/test-shards.txt, moved unchanged: #1586's balance run
```

One file, `<shard> <target>` lines as today. The script reads the last file by name.

```sh
# scripts/test-shards.sh <1-4>: shard n's cargo flags. The whole placement rule:
list=$(ls scripts/test-shards.d/*.txt | tail -1)
for t in $targets; do                                   # unit, then every crates/*/tests target, as today
  s=$(awk -v t="$t" '$2 == t && $1 ~ /^[1-4]$/ { print $1; exit }' "$list")
  [ "${s:-$(($(cksum <<<"$t" | cut -d' ' -f1) % 4 + 1))}" = "$1" ] || continue
  if [ "$t" = unit ]; then printf -- '--lib --bins '; else printf -- '--test %s ' "$t"; fi
done

# scripts/test-shards.sh balance <nextest output>...: today's weighing, written to a new file
new=scripts/test-shards.d/$(date -u +%F).txt
… | sort -k2 >"$new.tmp" && mv "$new.tmp" "$new"
find scripts/test-shards.d -name '*.txt' ! -path "$new" -delete
```

| a line or a target | today | after |
|---|---|---|
| a target the list does not name | `check` fails: "in no shard" | runs in shard `cksum(name) % 4 + 1` |
| a name listed twice | `check` fails: "in two shards" | the first line counts |
| a name with no test file | `check` fails: "no such test target" | ignored; the next balance run drops it |
| a shard outside 1 to 4 | `check` fails: "bad shard" | the line is ignored: the target takes its hash shard |

Deleted: `scripts/test-shards.txt`, the `check` form, the `lint` job's "test shards" step. The `test-shard` job's
call does not change. `crates/cli/tests/ci_split.rs` keeps its balance test and gains the table above as one test:
over a list with each kind of line, the four shards' flags together name every target once.

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
docs/serve/      page.md  report.md  alerts.md  webhooks.md  scene-listener.md  health.md  api-v1.md  screens.md
                 export.md
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
| the example's hygiene fragment | `docs/config.md#hygiene-1101` | the hygiene file |
| `crates/cli/tests/webhook.rs`, `crates/cli/tests/setup_neutral.rs` | read `docs/serve.md` | read the section's file |
| `crates/cli/tests/limits.rs` | compares the table in `docs/config.md` | the comparison is deleted |
| `.drover/rules.md`'s deps line, the `lint` job's deps step | the allow-list in `docs/serve.md` | the alerts file, which holds it |
| `.drover/docs.md`, first line | `docs/*.md`: one page per area | adds: four of them are a folder of sections |

## The page script (slice 6)

No code. `arch-check --write-baseline` runs once for the new `web/src` rule.
