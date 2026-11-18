# baseline: data shapes (#1099)

## fleet.toml

```toml
[fleet.drover]
baseline = "6e618cd22a43225e18a773b2dc5bd9ab24a9088b"   # absent = no baseline
```

## Types

```rust
/// config.rs: a full commit id; only 40 lowercase hex chars build one, so a bad key is a config error at load.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Baseline(String);

impl Baseline {
    pub fn sha(&self) -> &str;
    pub fn short(&self) -> &str;          // 12 chars, for lines people read
}

pub struct Fleet {
    // …
    /// #1099: lanes see history since this commit only; unset = all of it.
    pub baseline: Option<Baseline>,
}
```

```rust
/// baseline.rs
pub enum View {
    Since(Baseline),
    Full(Why),
}

pub enum Why {
    NoBaseline,
    CutBefore { head: String },           // the first head that does not contain the baseline
    Failed(String),                       // git's error; spawn logs `baseline-failed` with it
}

impl View {
    pub fn line(&self, branch: &str) -> String;       // spawn's output
    pub fn brief(&self) -> Option<String>;            // the brief's History line, for Since only
}

/// `drover baseline <fleet> [<sha>]`: resolve, check it is on integration, write the key, log `baseline-set`.
pub fn set(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, fleet: &str, sha: Option<&str>) -> Result<()>;

/// Clone level, idempotent, from any worktree of the clone: config first, a reset if the baseline changed, then the missing grafts.
pub fn grafts(r: &dyn Runner, f: &Fleet, dir: &Path) -> Result<()>;

/// Worktree level: `grafts`, then the switch. Never an error: a failure is `Full(Failed)`.
pub fn view(r: &dyn Runner, f: &Fleet, wt: &Path, heads: &[&str]) -> View;

/// The workflow page's "History baseline" section.
pub fn doc() -> String;
```

```rust
/// runner.rs: the argv a `git` really gets; every other program's is unchanged.
pub fn plain(prog: &str, args: &[String]) -> Vec<String>;
```

## Events

| kind | data | by |
|---|---|---|
| `baseline-set` | `sha`, `old` (or null), `by: owner` | the verb |
| `baseline-failed` | `agent`, `worktree`, `why` | spawn, when `view` returns `Full(Failed)` |

Both are born `.passive()`. No issue state reads either.

## What a clone holds

| Where | Key | Value | Meaning |
|---|---|---|---|
| shared config | `extensions.worktreeConfig` | `true` | worktrees may carry their own config |
| shared config | `core.useReplaceRefs` | `false` | by default nobody in this clone sees a graft |
| shared config | `drover.baseline` | the sha | which baseline the replace refs were built for |
| `refs/replace/<sha>` | | a parentless copy of the commit | one per boundary commit |
| a lane worktree's own config | `core.useReplaceRefs` | `true` | this lane sees history since the baseline |

Invariant: no replace ref exists in a clone whose shared config does not say `core.useReplaceRefs = false`. The
writes below are in the order that keeps it.

## `grafts`, as the git it runs

All through the runner, so every line also carries `--no-replace-objects`.

```sh
# 0. T must be a commit on integration, else stop with an error (no writes)
git -C $dir merge-base --is-ancestor $T origin/$integration

# 1. config, before any ref
git -C $dir config extensions.worktreeConfig true
git -C $dir config core.useReplaceRefs false

# 2. the baseline changed (drover.baseline is set and is not $T): views off, then refs
git -C $dir worktree list --porcelain            # for each: git -C <wt> config --worktree --unset core.useReplaceRefs
git -C $dir replace -d $(git -C $dir replace -l)
git -C $dir config drover.baseline $T

# 3. the missing grafts
git -C $dir rev-list --boundary HEAD origin/$integration ^$T     # each "-<sha>" line not in `replace -l`:
git -C $dir replace --graft <sha>
```

With the key absent and `drover.baseline` set: step 2's two deletes, then unset `drover.baseline` and
`core.useReplaceRefs`. `extensions.worktreeConfig` stays; it is inert.

## `view`, after `grafts`

```sh
# on only if every head contains T
git -C $wt merge-base --is-ancestor $T <head>           # for HEAD and each head the brief names
git -C $wt config --worktree core.useReplaceRefs true   # or `--unset` when one does not
```

## What "since the baseline" counts

```sh
git -C $wt log --oneline | wc -l
git -C $wt --no-replace-objects rev-list --boundary HEAD origin/$integration ^$T | wc -l     # the same number
```
