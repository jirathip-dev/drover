# Shapes

## Events

Two new kinds. Both are plain log events; neither is passive.

| Kind | `issue` | Data | Logged by |
|---|---|---|---|
| `epic` | none | `op` (`add`, `rm`), `parent`, `issues`, `title` (the parent's, on `add`), and the `thread` and `via` every such event may carry | `epic::change` |
| `epic-landed` | the parent | `Landed`: `title`, `merged` (`[{issue, pr, title}]`, issue order), `dropped` (`[issue]`); push flag set | `epic::settle`, once per parent |

```json
{"ts":1,"fleet":"acme-api","type":"epic","op":"add","parent":24,"issues":[11,12,13,15,17],"title":"Payments API v2"}
{"ts":9,"fleet":"acme-api","issue":24,"type":"epic-landed","push":true,"title":"Payments API v2",
 "merged":[{"issue":11,"pr":26,"title":"Refunds endpoint"},{"issue":12,"pr":28,"title":"Currency field"}],"dropped":[13]}
```

Why `epic` has no `issue`: the fold makes a `queued` state for any issue number an event names. A parent with a
state would show as a queued issue nobody drives. `queue` events are shaped the same way for the same reason. They
are never archived (compaction removes only a done issue's events), so the list outlives every child.

Reused, unchanged: `closed-other` (the owner closed the parent or a child on GitHub), `evidence-missing` (the
parent's comment failed twice; the parent stays open), and a `done` row of the #509 action log that makes
`epic-landed` once-only (`action_id` = `<fleet>#<parent>:epic-land`, no issue).

### The fold

| Event | Change to `IssueState` |
|---|---|
| `merged` | also sets the new `pr` (the event's `pr`, a string or a number in old logs) |
| `epic-landed` | the parent's state becomes `merged` |

`IssueState.pr: Option<u64>` is the one new field. It is saved with the state, so a child's PR number survives the
14-day archive of its events. A child merged and archived before this ships has none.

## Types (`epic.rs` in core)

```rust
/// Each open parent's children, in issue order: the `epic` events of one fleet, replayed.
pub type Members = BTreeMap<u64, BTreeSet<u64>>;

/// One open epic as every view shows it.
pub struct Epic {
    pub fleet: String,
    pub parent: u64,
    pub title: String,
    /// When its first child was added.
    pub since: u64,
    pub children: Vec<Child>,
}

/// A child, by the state its own issue has. `t` is when it got there.
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Child {
    Merged { issue: u64, pr: Option<u64>, t: u64 },
    Dropped { issue: u64, t: u64 },
    /// `state` as `drover status` words it (`queued`, `impl`, `review(1)`, `ready`, `blocked`, …) or `not queued`.
    Open { issue: u64, state: String, t: u64 },
}

impl Epic {
    pub fn merged(&self) -> usize;      // k
    pub fn total(&self) -> usize;       // n = children - dropped
    pub fn dropped(&self) -> usize;
    /// No child open and one or more merged.
    pub fn lands(&self) -> bool;
    /// The one line `drover waiting` and the compact board print.
    pub fn line(&self) -> String;
}

/// What `epic-landed` carries; the comment and the row's block are both written from it.
pub struct Landed {
    pub title: String,
    pub merged: Vec<Kid>,               // Kid { issue, pr: Option<u64>, title: String }
    pub dropped: Vec<u64>,
}

pub enum Op { Add(Vec<u64>), Rm(Vec<u64>) }
```

| A child's issue | Child |
|---|---|
| state `merged` | `Merged`, `pr` from its state |
| state `closed` (closed on GitHub, by triage, by a lane's end) | `Dropped` |
| any other state | `Open`, with that state |
| no state, in the queue | `Open { state: "queued" }` |
| no state, not queued | `Open { state: "not queued" }`: the epic waits on it and says so |

An epic is open while it has a child and its parent has no done state. A parent that reads `merged` or `closed` has
no epic, whatever the list says.

## The API

`GET /api/v1/queue`: each fleet card gains `epics: Epic[]` (the type above, written to the generated TypeScript
types by `just types`).

Thread rows: the landed row is `kind: "merged"`, `issue` the parent, `key` `epic-<fleet>-<parent>`, `open: false`,
`to` the parent's home (the thread whose orchestrator logged the first `add`, else the repo's desk). Every row gains
`epic`: null, or on the landed row the event's `Landed`, as logged:

```json
"epic": {"title": "Payments API v2", "dropped": [13],
         "merged": [{"issue": 11, "pr": 26, "title": "Refunds endpoint"}, {"issue": 14, "pr": null, "title": ""}]}
```

### Contract keys

`BLESS=1` appends these to `rows.keys` and `thread-rows.keys` (with the `rows[]` prefix) and to `row-merged.keys`
(with `[]`); every other `row-*.keys` and `rows-open.keys` gains only the `epic: null` line. Nothing is removed or
retyped.

```
[].epic.dropped[]: number
[].epic.merged[].issue: number
[].epic.merged[].pr: null
[].epic.merged[].pr: number
[].epic.merged[].title: string
[].epic.title: string
[].epic: null
```

The contract test's log gains one epic: two children merged (one `merged` event with a `pr`, one without), one
dropped, then the `epic-landed`.

### The Queue block

No new block type and no new key: a `section` (`epics-<fleet>`, header `epics (N)`) holding one `list` of `row`s,
as `docs/design/epics/` draws it. Built by `screens_epics.rs` from `QueueCard.epics`:

| Row | id | From |
|---|---|---|
| head | `epic-<fleet>-<parent>` | title `#<parent> <title>`, badge `k/n merged` (tone done), subtitle the bar and the counts, `value.since` = `since`, tap the parent's issue sheet |
| child, `Merged` | `ep-<fleet>-<n>` | figure state `done`, subtitle `merged · PR #<pr>` (`merged` alone with no `pr`), `value.since` = `t` |
| child, `Dropped` | same | grey `dropped` badge, subtitle `closed unmerged · not counted` |
| child, `Open` | same | the issue's own Queue row (`screens::row`), re-ided and indented; `not queued` as a plain subtitle |
| fold | `epic-fold-<fleet>-<parent>` | the chain fold's link and `fold` query, key `<fleet>-<parent>` |

The screens vocabulary test (`cooee_screens.rs`) runs over the Queue with an epic in it and so proves the claim.

## Texts

| Where | Text |
|---|---|
| `drover waiting`, compact board | `epic <fleet> #<parent> <title> · <k>/<n> merged · <i> in progress · <q> queued · <d> dropped · open #a #b (#c dropped)`; zero counts left out |
| `events::plain` of `epic-landed` (the row's text, the push body, a notifier's line) | `<fleet> #<parent> <title> landed: <k> issues`, then ` · <d> dropped` when any |
| web push title | `#<parent> landed`, under the device's `merged` word |
| the comment and, under the text line, the row's block | `Landed: <k> of <k> issues merged.`, a line `- #<n> <title>: PR #<pr>` per merged child, then `Dropped (closed unmerged): #<n> …` when any |

## Refusals (exit 3)

| Code | When |
|---|---|
| `EPIC_PARENT` | `epic add`: the parent is closed, merged, queued or has had a lane, is a child of an epic, or is one of the children named; a child named is itself an open epic's parent. `queue add`, `spawn`: the issue is an open epic's parent |
| `EPIC_CHILD` | `epic add`, `queue add --epic`: a child is already in another open epic; it names the `rm` that frees it |
| `EPIC_DONE` | `epic add`: after it no child would be open (all merged or closed): there is nothing to track |
| `IS_PR` (existing) | the parent or a child is a pull request |
| `ORCHESTRATOR_ONLY` (existing, the hook) | a lane other than an orchestrator runs `epic` or `queue add --epic` |

Adding a child twice, or removing one that is not there, logs nothing and exits 0.
