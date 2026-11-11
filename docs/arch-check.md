# arch-check (#734)

`herdsman arch-check [--config .herdsman/arch.toml]` is a fast static check every fleet repo runs in CI. It scans
Rust and TypeScript sources (and any file a `[[ban]]` names) as plain text (no compiler) and prints each violation as `file:line: why`, exiting 1 on
any. A repo with no config has nothing to check (exit 0). Paths are globs from the repo root (`**` any depth, `*`
inside one folder); `target*`, `node_modules` and dot folders no `[[ban]]` path starts with are skipped.

```toml
# A layer's files, the names other code uses it by, and the layers it may use. Rust: any `a::b` path on a line
# (and one `use a::{b, c}` group) under a `mods` name. TypeScript: an `import`/`export … from`/`require` module,
# a relative one resolved to a path and matched against `paths` (with `.ts`, `.tsx`, `/index.ts`).
[[layer]]
name = "core"
paths = ["crates/core/src/**"]
mods = ["herdsman_core"]
may_use = []
exclude = ["**/*.test.*", "**/__tests__/**"] # files this entry does not check: non-test source only

[[layer]]
name = "server"
paths = ["crates/server/src/**"]
mods = ["herdsman_server"]
may_use = ["core"]

# A substring that may appear only in its exceptions, in any file its paths match (`.yml`, `.mjs`, `.sql`, …).
# `//` comment lines (in Rust, TypeScript, JavaScript and other `//` languages) are skipped unless `comments = true`.
[[ban]]
pattern = "Command::new"
paths = ["crates/*/src/**"]
except = ["crates/core/src/runner.rs"]
reason = "#713: spawn processes through the Runner"

[[ban]]
pattern = "@ts-expect-error"
paths = ["web/src/**"]
reason = "type it instead"
comments = true

[[ban]]
pattern = "run: exit 1"
paths = [".github/workflows/*.yml"]
reason = "a failing step says why"

# Size limits per glob. A file or function over one fails unless the baseline lists it at that size or more,
# so only new or grown offenders fail.
[[size]]
paths = ["crates/*/src/**/*.rs"]
max_file_lines = 2000
max_fn_lines = 150
exclude = ["**/tests/**"]

baseline = ".herdsman/arch-baseline.txt" # the default
```

`herdsman arch-check --write-baseline` writes today's offenders to the baseline, one `<file> <lines>` or
`<file>:<fn> <lines>` a line, and today's layer crossings, one `<file> -> <layer>` a line: a listed crossing passes,
any other fails. Once the baseline lists crossings they may only shrink: `--write-baseline` drops fixed ones and
exits 1 naming each new one instead of adding it. A function runs from its `fn` / `function` / `const f = (…) => {` line to the first
later line at its indent starting with `}`, so the source must be formatted.

## Comments and docs (#735)

```toml
# A comment line added since --base fails (Rust and TypeScript `//`, `/* */`), but `// SAFETY:` right above an
# `unsafe` line and one `///` or one-line `/** */` on a `pub`/`export` item. Old lines are never judged.
[comments]
policy = "off" # "on": no check
except = ["assets/types/**"] # generated files

# Every repo path, `path:line`, link and (with cli = true) `herdsman …` command and flag in these docs must exist.
# With --pr-body, a changed `name:` line in a watched file needs a changed doc line naming it (`name` or `name-with-dashes`),
# or `no doc: <why>` in the PR body.
[docs]
paths = ["docs/**/*.md", "README.md"]
watch = ["crates/core/src/cli.rs", "crates/core/src/config.rs", "crates/server/src/api.rs"]
skip = [".herdsman/**"] # paths that are each fleet repo's, not this one's
cli = true
```

A path counts when its first folder exists at the repo root (`../` ones from the doc's folder); fenced blocks are
skipped. CI runs `herdsman arch-check --base HEAD^1 --pr-body <body>` on a pull request (checkout `fetch-depth: 2`,
so `HEAD^1` is the base). The reason for a non-obvious line goes in the commit's why note (`herdsman why`), never in a
comment.

The lesson lane turns a repeated layer crossing or banned call into a `[[layer]]` or `[[ban]]` entry.

Another repo's CI runs it with the reusable workflow:

```yaml
jobs:
  arch-check:
    uses: <owner>/herdsman/.github/workflows/arch-check.yml@staging
```

## Playbooks: `--perf` and `--refactor`

- `queue add N --perf`: impl's first commit (message starting `baseline:`) records the measuring command and its
  numbers; rev's PASS carries a `Perf:` block with before/after by the same command, the improvement and the
  benchmark checklist (repeat runs, noise). Without either, `herdsman verdict` refuses the PASS `NO_BASELINE`.
- `queue add N --refactor`: behaviour is unchanged, so tests only move or get renamed. A PASS on a PR whose test
  files (`test`/`tests`/`__tests__` folders, `.test.`, `.spec.`, `_test.` names) gain or lose lines other than
  moved ones is refused `TESTS_CHANGED`.
