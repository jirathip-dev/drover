# Shapes

Sketches, not final code. Names may change in review; the split between them may not.

## The placing function

The whole design is this one total function over old commits. `line` is the fleet's integration branch read
along first parents; `cut` is a commit on it.

```text
place(c) =
  root              if c is the cut or an ancestor of it                       kind: root (the cut itself: same)
  line'(c)          if c is on the line after the cut                          kind: same
  place(C)          if c was merged into the line by C, the first line commit  kind: held
                    after the cut that reaches c and whose first parent does not
  copy(c)           otherwise: tree, author, committer and message of c;       kind: same
                    for each parent p: copy(p) if p is copied, place(p) if that
                    is tree-equal (kind same), else no parent
```

`same` pairs are one to one and tree-equal; they are the only rows undo reads backwards. A ref moves to
`place(tip)`. With open question 2 answered "delete", a branch whose tip is `root` or `held` is deleted
instead.

**Why a copy drops a parent.** A parent placed as `root` or `held` is a base whose tree no longer exists on
the line. Keeping that edge would hand a later merge the wrong base, and the merge would silently revert
work. Without it git refuses the merge ("unrelated histories"). So a branch that last met the line before the
cut is carried as a **detached** snapshot: every file kept, not mergeable. A branch that has merged the line
since the cut keeps its true base: the merge base before and after have the same tree, so its diff and its
merge are what they were.

## Values

```rust
/// 40 lowercase hex, checked where it enters: `--cut`, `--push`, git's output, the plan file.
pub struct Sha(String);

/// The old repo, read once from the scratch copy. Planning sees nothing else.
pub struct Old {
    pub commits: HashMap<Sha, Commit>,
    pub refs: Vec<(String, Sha)>,            // heads, tags, notes, and each PR head as pull/<n>
    pub notes: Vec<(String, Sha, String)>,   // notes ref, annotated commit, text
    pub open: Vec<(u64, String)>,            // open PR number, head branch (one `gh pr list`)
}
pub struct Commit { pub tree: Sha, pub parents: Vec<Sha>, pub author: String, pub committer: String, pub message: String }

pub enum Kind { Same, Held, Root }

/// What `plan` decides, before any object exists: new commits by index, parents by index.
pub struct Shape { pub nodes: Vec<Node>, pub place: HashMap<Sha, (usize, Kind)>, pub refs: Vec<(String, Sha, Option<usize>)> }
pub struct Node { pub tree: Sha, pub parents: Vec<usize>, pub author: String, pub committer: String, pub message: String }

pub fn plan(old: &Old, line: &str, cut: &Sha) -> Result<Shape, Refusal>;      // pure

/// The shape with git's ids filled in: what the dry run prints, the card pins and the push sends.
pub struct Plan {
    pub repo: String,
    pub line: String,
    pub cut: Sha,
    pub root: Sha,
    pub tips: (Sha, Sha),                    // the line's old and new tip; the new one is the run's id
    pub chain: Vec<Step>,                    // after the cut, oldest first
    pub refs: Vec<RefMove>,
    pub issues: Vec<IssueMove>,              // the events to append
    pub counts: Counts,
}
pub struct Step { pub old: Sha, pub new: Sha, pub subject: String, pub pr: Option<u64>, pub held: usize }
pub struct RefMove { pub name: String, pub old: Sha, pub new: Option<Sha>, pub class: Class, pub open_pr: Option<u64> }
pub enum Class { Line, SameTree, Carried, Detached, Merged, Notes }
pub struct IssueMove { pub fleet: String, pub issue: u64, pub moved: Vec<Moved> }
pub struct Moved { pub from: Sha, pub to: Sha, pub held: Vec<Sha> }
```

New commits are written with `git hash-object -t commit -w --stdin` through the runner's stdin call: no
environment, no clock, so the bytes and the id depend only on the plan.

## Files

```text
<state dir>/compact/<repo>/old.git           scratch mirror of origin, PR heads included; new objects are written here
<state dir>/compact/<repo>/plan.json         the Plan
<state dir>/compact/<repo>/compact-map.tsv   every old commit

<owner>/<repo>-history  (private)
  refs/heads/*, refs/tags/*, refs/notes/*    as they were
  refs/heads/pull/<n>                        each PR head (GitHub refuses pushes under refs/pull)
  refs/heads/drover-compact                  one commit: plan.json, compact-map.tsv
```

`compact-map.tsv`, one line per old commit, sorted by old id:

```text
<old sha>\t<new sha>\t<same|held|root>
```

## Events

Appended in one transaction by the approved run, all flagged passive (not lane activity). `run` is the line's
new tip.

```json
{"type": "compacted", "fleet": "cooee", "run": "3d4e…", "repo": "<owner>/cooee", "line": "staging",
 "cut": "8957…", "root": "77aa…", "old_tip": "b758…", "archive": "<owner>/cooee-history",
 "commits": [1081, 132], "refs": 361, "map_sha256": "…"}

{"type": "compacted", "fleet": "cooee", "issue": 310, "run": "3d4e…",
 "moved": [{"from": "9f8e…", "to": "0a1b…", "held": ["51aa…", "c07e…", "e2d4…"]}]}

{"type": "compact-undone", "fleet": "cooee", "run": "3d4e…"}
```

- A landed issue has one `moved`: `from` its merge commit, `held` the lane heads its verdict, why and merge
  records name.
- An open issue has one `moved` per recorded head that was carried, `held` empty.
- An issue landed before the cut has none: its ids are archive ids and still match there.
- Both kinds are added to the list of kinds the working log always keeps (`crates/core/src/events.rs:692`).

## The bridge

```rust
pub struct Shas { /* run → in force or undone; from/held → to; to → from */ }

impl Shas {
    pub fn of(events: &[Event]) -> Shas;
    /// The commit that holds `stored` today: itself unless a compact in force moved it,
    /// or an undone compact made it.
    pub fn now<'a>(&'a self, stored: &'a str) -> &'a str;
    /// `sha` is a compact root: the line is older than the cut.
    pub fn root(&self, sha: &str) -> Option<&Run>;
}
pub struct Run { pub repo: String, pub line: String, pub cut: Sha, pub root: Sha, pub archive: String }

/// `rev` in `dir` stands on the newest compact root of `f`'s repo; checked with git once per clone and run.
pub fn rooted(ctx: &Ctx, r: &dyn Runner, events: &[Event], f: &Fleet, dir: &Path, rev: &str) -> Result<(), Refusal>;
```

A fleet that was never compacted has no such events: `now` is the identity and `rooted` returns at once.

## The verb

```text
drover compact <FLEET> --cut <DATE|SHA> (--dry-run | --thread <ID>)
drover compact <FLEET> --undo (--dry-run | --thread <ID>)
drover compact <FLEET> [--undo] --push <TIP>       only inside its approved card
drover compact <FLEET> --reset                     this host's clone and lane folders, and every host that answers
```

## Refusals

| code | when |
|---|---|
| `COMPACT_NOT_READY` | a fleet of the repo is not paused; a lane is working; a lane folder is dirty or unpushed; a landing waits on its suite; the fleet has a `baseline`; an open PR comes from a fork or would be detached (update or close it); force pushes are not allowed. Every reason is listed, not only the first. |
| `COMPACT_TREE` | a line, same-tree, carried or detached ref would get a different tree. Nothing was sent. |
| `COMPACT_MOVED` | a ref on origin is at neither the plan's old nor its new value (undo: not at the new one). Names the refs. |
| `COMPACT_ARCHIVE` | the archive is public, holds other content, or its read-back misses a ref. Nothing was pushed to origin. |
| `COMPACT_UNAPPROVED` | `--push` outside a run card that is approved, started and not done, with exactly this command. |
| `OLD_ROOT` | drive, spawn or merge: a clone, lane folder or PR head not on the new root (an old clone, or a detached branch). Names `--reset` for a clone or folder. |
