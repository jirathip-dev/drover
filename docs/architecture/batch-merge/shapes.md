# batch-merge: the shapes

## Config

```rust
pub struct Fleet {
    /// The most PRs one merge stacks; unset or 1 = one at a time.
    pub batch_max: Option<usize>,
}

impl Fleet {
    /// The riders one merge may take: `batch_max − 1`, 0 when batching is off.
    pub fn riders(&self) -> usize
}
```

| At load (`Config::parse`) | Result |
|---|---|
| `batch_max = 0` | error: `fleet.<n>: batch_max is at least 1` |
| `batch_max > 1` with `[fleet.<n>.land] mode = "fast-first"` | error: `fleet.<n>: batch_max and fast-first landing together; keep one` |

One new `[limits]` key, a row in `limit_docs.rs`, so `docs/config/limits.md` comes from `just gen`:

| Key | Default | What it times |
|---|---|---|
| `batch_settle_secs` | 30 | how long a batch waits for GitHub to say the next PR is mergeable after the one before it landed |

## Types (`crates/core/src/batch.rs`)

```rust
/// A PR that passed every merge gate at `head`: what `gate::eligible` returns.
pub struct Ready {
    pub issue: u64,
    pub pr: Value,
    pub num: String,
    pub head: String,
    pub passed_at: u64,
}

/// Why a lead takes no riders.
pub enum Alone {
    Off,
    NotReady,
    Serial(String),
    StrictRule,
    NoneReady,
}

/// Integration's tip with each member merged on top, in order; lives in the scratch worktree only.
pub struct Stack {
    base: String,
    layers: Vec<Layer>,
    dropped: Vec<u64>,
}

/// One member on the stack: the local merge commit and the tree of everything up to it.
pub struct Layer {
    pub member: Ready,
    pub commit: String,
    pub tree: String,
}

/// What the gate said of a stack. `at` is the first layer whose prefix is red; every layer under it is green.
pub enum Checked {
    Green,
    Culprit { at: usize, check: Check, output: String },
}

/// How a landing ended.
pub enum Landed {
    All,
    Stopped { landed: usize, why: Stop },
}

pub enum Stop {
    TipMoved,
    HeadMoved,
    TreeDrift,
    Unsettled,
    Refused(String),
}
```

What the types rule out:

- A `Stack` has at least one layer and the lead is layer 0: `Stack::build` returns the lead's `NOT_MERGEABLE`
  instead of a stack without it.
- `Checked::Culprit` can only be built from a red run of prefix `at + 1` and a green run (or the base) under it,
  so a PR is charged on a run of its own prefix, never on the top's.
- A `Ready` exists only out of `gate::eligible`, so nothing lands that skipped a gate.

## The batch id and the run cache

- The batch id is the tree of the stack's top: the same members on the same base give the same id.
- A gate run is logged with the tree it ran on. Before a run, `check` looks for that tree in the log:
  one green run makes a tree green for good. The top is red after two red runs; a prefix after one.
- This is all the resume there is. A batch that died re-plans; where its stack is the same, its runs are read
  back, not run again. The PRs left by a stopped landing stack to a tree that was already green.

## Events

All three are fleet events (`issue: null`), logged `.passive()`: no issue changes state on them.

```json
{"kind": "batch-planned", "data": {"batch": "<tree>", "base": "<sha>", "lead": 12,
  "members": [{"issue": 12, "pr": "1912", "head": "<sha>"}], "dropped": [{"issue": 21, "why": "conflict"}]}}

{"kind": "batch-checked", "data": {"batch": "<tree>", "tree": "<tree>", "upto": 3, "green": true,
  "secs": 412, "check": null}}

{"kind": "batch-landed", "data": {"batch": "<tree>", "outcome": "landed | charged | stopped | flaky",
  "landed": [12, 15], "left": [19], "culprit": null, "why": null}}
```

- `batch-planned` and `batch-landed` are logged only for a stack of two or more.
- `outcome: "flaky"` is a landed batch whose top was red once and green on the retry; it carries the check's name.
- `merged` gains `batch` for a PR that landed in one. The lead's carries `premerge_secs`, as today.
- The culprit's `premerge-failed` and drive `verdict FAIL` are today's, with `batch` beside.
- Nothing is added to `crates/core/src/events/passive.rs` (frozen): each emit site sets the flag.

## What is not stored

No batch table, no batch state in `fold`, no new issue state. An issue is `ready` until its own `merged`.
The scratch worktree is `premerge-<fleet>`, today's, reset per build.
