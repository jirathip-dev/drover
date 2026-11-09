# arch-check (#734)

`herdsman arch-check [--config .herdsman/arch.toml]` is a fast static check every fleet repo runs in CI. It scans
Rust and TypeScript sources as plain text (no compiler) and prints each violation as `file:line: why`, exiting 1 on
any. A repo with no config has nothing to check (exit 0). Paths are globs from the repo root (`**` any depth, `*`
inside one folder); dot folders, `target*` and `node_modules` are skipped.

```toml
# A layer's files, the names other code uses it by, and the layers it may use. Rust: any `a::b` path on a line
# (and one `use a::{b, c}` group) under a `mods` name. TypeScript: an `import`/`export … from`/`require` module,
# a relative one resolved to a path and matched against `paths` (with `.ts`, `.tsx`, `/index.ts`).
[[layer]]
name = "core"
paths = ["crates/core/src/**"]
mods = ["herdsman_core"]
may_use = []

[[layer]]
name = "server"
paths = ["crates/server/src/**"]
mods = ["herdsman_server"]
may_use = ["core"]

# A substring that may appear only in its exceptions (comment lines are skipped).
[[ban]]
pattern = "Command::new"
paths = ["crates/*/src/**"]
except = ["crates/core/src/runner.rs"]
reason = "#713: spawn processes through the Runner"

# Size limits per glob. A file or function over one fails unless the baseline lists it at that size or more,
# so only new or grown offenders fail.
[[size]]
paths = ["crates/*/src/**/*.rs"]
max_file_lines = 2000
max_fn_lines = 150

baseline = ".herdsman/arch-baseline.txt" # the default
```

`herdsman arch-check --write-baseline` writes today's offenders to the baseline, one `<file> <lines>` or
`<file>:<fn> <lines>` a line. A function runs from its `fn` / `function` / `const f = (…) => {` line to the first
later line at its indent starting with `}`, so the source must be formatted.

The lesson lane turns a repeated layer crossing or banned call into a `[[layer]]` or `[[ban]]` entry.

Another repo's CI runs it with the reusable workflow:

```yaml
jobs:
  arch-check:
    uses: jirathip-dev/herdsman/.github/workflows/arch-check.yml@staging
```

## Playbooks: `--perf` and `--refactor`

- `queue add N --perf`: impl's first commit (message starting `baseline:`) records the measuring command and its
  numbers; rev's PASS carries a `Perf:` block with before/after by the same command, the improvement and the
  benchmark checklist (repeat runs, noise). Without either, `herdsman verdict` refuses the PASS `NO_BASELINE`.
- `queue add N --refactor`: behaviour is unchanged, so tests only move or get renamed. A PASS on a PR whose test
  files (`test`/`tests`/`__tests__` folders, `.test.`, `.spec.`, `_test.` names) gain or lose lines other than
  moved ones is refused `TESTS_CHANGED`.
