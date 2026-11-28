# ADR: epics — a parent issue, its children, `k/n merged`, one landed row (#1786)

## Summary

Decision: an epic is a list of child issues under a parent issue, kept as events. Drover never builds the parent,
and every view counts the children from the states it already keeps: merged, dropped (closed unmerged) or open.
When none is open and at least one merged, drover logs one pushed landed row, writes one note on the parent that
links each child's PR, and closes it.
Slices: one PR, three commits: the list with its verb and the queue flag; the views; the landing and contract keys.
Open questions: one. The last merge sends the child's push, then the epic's. Keep both (recommended) or only one?

## Status

Proposed (architect lane, 2026-11-25, staging `c73f4b6f4`). Read `README.md` first, then this, `shapes.md`,
`boundaries.md`. The owner's decision q3758 asked for the data, the events, the close rule and the contract keys,
with the block built from the screens kit. the design draft (dropped by the 2026-11-25 amendment) is a draft of the screens, not yet approved; where
this ADR departs from it is listed below.

## Question

How does drover know that a set of issues is one feature, show `k/n merged` wherever the queue is shown, and tell
the owner once and close the parent when all of it has landed, without a second store, without a new issue state
every reader must learn, and so that a child closed unmerged never holds the feature open?

## Where it stands

- Issues are linked only by queue blockers. A blocker is an order, not a group: it drops out of view once merged.
- Each issue's state is folded from the log and saved (`IssueState`). `merged` and `closed` are its two ends.
  Compaction archives a done issue's events after 14 days and keeps the saved state, so a reader that scans for
  `merged` events is already wrong for old issues. The saved state has no PR number.
- Any event that names an issue number gives that issue a state, `queued` to begin with.
- `queue` events name no issue (`op`, `issues` in the data) and are replayed into the queue on every read.
- A merge is: the `merged` event (pushed), one evidence comment (tried twice, else `evidence-missing` and the issue
  stays open), then `gh issue close`. The #509 action log makes a step once-only across passes.
- Drive already asks GitHub, once a minute and in one call, which open issues it knows have closed
  (`closed-other`).
- A thread row is a pure function of one event. Its kind `merged`, its `block` and its push come from the event.
  `RowView` adds typed extras per row (`handoff`). The rows contract is append-only.
- The Queue screen is built from `QueueCard` in typed blocks; `screens.rs` is at 1,985 of its 2,000 lines.
- The flow table (#1649) holds rows for things that have an actor and a clock.

## Options

### 1. Where the list of children lives

| Option | Why not |
|---|---|
| A. On GitHub: sub-issues, a task list in the parent's body, or a label | read over the network for every view (never allowed on serve's request path), or cached and stale; two writers; the issue asks for events |
| B. A mark on the queue item (`queue set <n> epic <p>`) | only queued issues have an item; an item leaves the view when its issue merges, which is when the epic needs it |
| C. `epic` events that name the parent as their issue, the parent in a new state `epic` | every reader of states (status, board, waiting, drive, the flow table's "no state without a row" test, SLOs) must learn a state that is never built |
| **D. `epic` events with no issue number, replayed like the queue (chosen)** | — |

D adds a list and no state. The parent has no state at all while its epic is open, so no issue code changes. The
events are never archived, as `queue` events are not.

### 2. Where `k/n` comes from

| Option | Why not |
|---|---|
| An event per child outcome (`epic-child-merged`, …), counted | a second copy of facts the log has; wrong the first time a child merges outside drover, is reverted or respawned |
| **Derived on every read from each child's saved state (chosen)** | — |

One gap had to close for this: the saved state gains `pr`, set by the `merged` event, so a child that merged a
month ago still has its PR for the comment.

### 3. How the landing is recorded

| Option | Why not |
|---|---|
| A. A `merged` event on the parent | every reader of `merged` takes it for a PR merge: the autonomy counts, "done today", `why`, hotspots, rollouts |
| C. A new kind with no issue number | the row has no issue and no home thread without special cases; a blocker on the parent never releases |
| **B. A new kind `epic-landed` on the parent, folded to the state `merged` (chosen)** | — |

With B the parent ends as any landed issue does. Readers of the state `merged` already cope with a missing `merged`
event, because compaction removes it. The thread row is a `merged` row, as the issue asks.

### 4. When the landing is checked

| Option | Why not |
|---|---|
| Only right after a merge | misses the epic whose last open child is dropped or removed |
| Only in the drive pass | the landing comes up to a pass after the merge |
| **One idempotent `settle`, called after a merge and in every drive pass (chosen)** | — |

Not a flow-table row: an epic has no actor and no clock. It waits on its children, and each child's own rows drive
it. A statechart is in `boundaries.md`.

### 5. The contract keys

| Option | Why not |
|---|---|
| A new row kind `epic` | the issue asks for a `merged` row; a new kind is a new thing for every client to draw |
| An open epic as an open row | open rows are what waits on the owner; an epic waits on nobody |
| A `merged` row and nothing else | a client cannot tell a landed feature from one merge without reading the text |
| **A `merged` row with one typed extra, `epic` (null on every other row), as `handoff` is done (chosen)** | — |

The Queue block needs no key: it is a section of rows.

## Decision

1. **The list.** `epic` events: `add` and `rm` of children under a parent, replayed per fleet. A child is in at
   most one open epic. A parent is never queued or spawned, and is no one's child. Refusals in `shapes.md`.
2. **The verbs.** `epic <fleet> <parent> add <n>…`, the same with `rm`, and `queue add --epic <parent>`. The owner
   and orchestrators only; the lane hook refuses others. `add` reads the parent and each new child once from
   GitHub (an open issue, not a PR) and keeps the parent's title in the event.
3. **The view.** One function, `epic::open`, returns each open epic with every child classed `Merged` (with its
   PR), `Dropped` or `Open` (with the issue's state, or `not queued`). The Queue section, the compact board's
   line, `drover waiting`'s line and `GET /api/v1/queue` all use it.
4. **The count.** `k` is the merged children, `n` the children not dropped. A dropped child is listed and named,
   and leaves the count: `2/2 merged · 1 dropped`. The AC's "dropped, not merged" decides this.
5. **The close rule.** An epic lands when no child is open and at least one is merged. If every child is dropped it
   stays open: drover does not close a feature of which nothing landed. If the owner closes the parent by hand, the
   epic ends with nothing said.
6. **The landing.** `epic::settle`: `epic-landed` once (the #509 action log), pushed; then one comment on the
   parent, tried twice; then the close. The same order and the same failure (`evidence-missing`, parent left open)
   as a merge.
7. **The row.** Kind `merged`, key `epic-<fleet>-<parent>`, in the parent's home thread, with `epic` carrying the
   merged children with their PRs and the dropped ones.
8. **`add` never lands an epic.** An `add` after which no child would be open is refused `EPIC_DONE`. Without
   it, naming five merged issues would close the parent and push from a bookkeeping command.

## Consequences

- `queue block <n> --on <parent>` now means "after the whole epic": the parent reads `merged` when it lands. No
  code is written for this.
- A child the owner finishes by hand and closes counts as dropped. Merged means drover recorded the merge.
- A child that is in an epic but not queued shows as `not queued`, and the epic waits on it. That is the reminder.
- A child merged more than 14 days before this ships has no saved PR: its comment line names the issue only.
- A long epic is safe: nothing read for the count or the comment is ever archived while the epic is open.
- The landed epic is not on "done today": that list is PR merges, with a sha each. The row and the push tell it.
- A notifier with its own `on` list gets `epic-landed` only when the list names it. A phone with no list gets it.
- `drover waiting --json` is unchanged; machine readers take `epics` from the queue route.
- An `rm` of the last open child lands the epic on the next pass. That is the owner dropping it by another road.
- Cost: about +370 lines of `src/` (the module about 220, the Queue section about 80, the rest one to six lines per
  touched file). Headroom is 437 today and about 2,300 once the open PRs land; the impl lane states its number.

## Departures from the design draft

| Draft (the design draft, dropped from the PR by the 2026-11-25 amendment) | Here | Why |
|---|---|---|
| The epic's push replaces the last child's | both are sent (the open question) | the child's push leaves before the epic is looked at |
| Push title `acme-api #24 landed`, body without the fleet | the row's generic push: title `acme-api #24`, body the row's text | no second wording path |
| Row key `merged-epic-<fleet>-<n>` | `epic-<fleet>-<n>` | the shape `rollout-<fleet>-<n>` already has |
| A golden `row-epic.keys` of the Queue head row | the keys of the row's `epic` extra, in the existing `.keys` files | the contract test goldens rows by kind; the Queue block adds no key |
| Open question 1, the count | decided: dropped leaves the count | the AC |
| Open question 2, "done today" | decided: not listed | above |

The screens themselves stand as drawn.

## Slices

One PR, three commits, each green.

1. **The list.** `epic.rs` (`replay`, `check`, `change`, `parent_of`, `is_parent`), the `epic` verb, `queue add
   --epic`, the three refusals, the hook line, `Registry::add`'s arm.
2. **The views.** `Child`, `Epic`, `open`, `line`; `IssueState.pr`; `watched` in the closed-issues check;
   `QueueCard.epics`; `screens_epics.rs` and its one call; the compact board and `drover waiting`.
3. **The landing.** `settle`, `summary`, `events::emit_once`, the fold arm, `plain`, the row kind and key,
   `RowView.epic`, the web push title, the two call sites; the contract keys; the docs (a new `epics.md` page under
   docs, one line in `docs/rows.md` and in the queue row of `docs/serve/api-v1.md`, the usage doc's verb and codes).

## Tests

In one new file under the CLI crate's tests, on the real entry points with the fake runner:

| Test | Asserts |
|---|---|
| an epic of three shows 2/3, and the third merge closes the parent once | after two merges `drover waiting` and the Queue screen say `2/3 merged`; after the third the runner saw one comment and one close on the parent, the log has one `epic-landed`, the rows route has one row keyed `epic-<fleet>-<parent>` with a push; a further drive pass adds none of them |
| a child closed unmerged is dropped and does not block the close | with one child `closed-other` and two merged: the child reads `dropped`, the count `2/2`, the epic lands, the comment names the dropped child |
| the refusals | `EPIC_PARENT`, `EPIC_CHILD`, `EPIC_DONE`, and a repeated `add` logging nothing |

And two existing tests widened: `contract_v1.rs` (its log gains an epic; `BLESS=1` appends the keys of
`shapes.md`) and `cooee_screens.rs` (the Queue with an epic passes the vocabulary check: no new block key).

## Open question

**Two pushes or one on the last merge.** Today `merged` is pushed where it is logged, before anything looks at the
epic. Recommended: keep both; the second says something the first does not. If one is wanted: `gate::merged`
asks `epic` whether this merge completes an epic and puts a flag on the `merged` event; the phone push and the
row's push skip a flagged merge, other notifiers and the thread row do not. About six lines. When the last two
children merge in the same second neither is flagged, and it falls back to a push for each merge and the epic's.

## `.drover/arch.toml` lines

No file is frozen, collected or generated anew. Every hot file this touches is on the `shared` list already, and
each gets one to six lines. The PR adds the two bans below and names the new screens file in the #1342 `json!` ban.

```toml
[[ban]]
pattern = "crate::spawn"
paths = ["crates/core/src/epic.rs"]
reason = "#1786: an epic starts nothing: it reads states and closes its parent"

[[ban]]
pattern = "crate::drive"
paths = ["crates/core/src/epic.rs"]
reason = "#1786: an epic starts nothing: it reads states and closes its parent"

[[ban]]
pattern = "json!"
paths = ["crates/server/src/screens.rs", "crates/server/src/screens_kit.rs", "crates/server/src/screens_lanes.rs", "crates/server/src/screens_epics.rs", "crates/server/src/serve/chat.rs"]
reason = "#1342 (q2063): screens are built through blocks' typed kinds and ops, never a hand-written json! literal"
```

The third block replaces the existing `json!` ban; only its `paths` line changes.
