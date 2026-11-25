# test diet: how the callers read (#1760)

Sketched before anything is built. Each block is what someone sees or writes once the slices have landed.

## A slice under gatekeeper's ratchet

Nothing here is built by the diet: this is gatekeeper's `core/test-health`, as its ADR (gatekeeper#99) prints
it, once drover has adopted it. A lane's run and CI print the same:

```
ok    core/test-health     0.2s /    5s
      tests 68,410 / src 52,300 lines: ratio 1.31, ceiling 1.35
```

A delete slice takes the ratio a whole step under the ceiling, and the check asks for the ratchet's turn:

```
FAIL  core/test-health     0.2s /    5s
      ratio 1.28 is a step under the ceiling 1.35: lower it (./.gatekeeper/run baseline core/test-health)
$ ./.gatekeeper/run baseline core/test-health
ci/tests/ratio.d/2026-12-09-130.toml: ceiling 1.30 (was 1.35)
```

The slice commits that one file. That is all a diet slice ever does about the cap. A slice that lands before
adoption does nothing: the first ceiling is written at adoption, from the ratio the slices left.

A lane whose feature PR goes over the ceiling reads one line and has two moves: add the case as a row of the
table that already tests the verb, or delete a test its change made redundant, with its ledger line.

## A slice that rewrites a slow test's file

Also gatekeeper's, once adopted. A change to a helper of a test file changes every test in it, and a changed
test over 2 s fails:

```
FAIL  local/test          48.1s /  600s
      over 2 s: drover::design_stage revise_redoes_the_design_with_the_note_in_its_brief_five_times_in_a_row took 5.1 s at its fastest of 3 (changed in this change)
```

The slice makes the test faster (the causes the audit found: a process-wide lock, a socket waiting out a
timeout, a real sleep), or moves its full size into the `soak` target and leaves a small check of the same rule
where it was.

## A PR that deletes tests

Its body carries the ledger (`shapes.md`, section 2):

```
## Tests removed

- `api_v1_ops::setup_starts_a_configured_terminal_and_reads_its_screen` → kept by `setup_terminal::the_board_starts_by_name_types_unlogged_and_shows_the_screen` (dup)
- `history::the_events_api_reads_a_seq_window` → kept by `serve_working_set::board_routes_read_the_working_set_and_a_merged_issues_shot_still_serves` (dup)
- `serve_wire::wire_report` → none (impl: prints bytes and times, asserts a 200)

Kept-by: run `setup_terminal::the_board_starts_by_name_types_unlogged_and_shows_the_screen`
Change: crates/server/src/serve/v1.rs: `setup` answers before it starts the terminal
```

then the command and its failing output, fenced.

The keep job runs because the diff removes a `#[test]`:

```
tests removed: 3, in the ledger: 3
keepers and rows found at the head: 2 of 2
src lines covered at the base 31,204, lost at the head: 0
```

The reviewer reads the ledger against the inventory, re-runs three `Kept-by` blocks, and records the verdict.

## A PR that only moves tests

Slices 6 to 11 rewrite fixtures and move files. They remove no test name, so the body says:

```
## Tests removed

none
```

and the keep job agrees: `tests removed: 0`. A test that vanished in the move fails the job by name.

## A test on the builders

Today, the first half of a test in `drive.rs`, as it stands. The whole test is 36 lines; its helpers `setup`,
`lane`, `sha`, `ev`, `fake`, `draft_pr`, `pr`, `drive` and `state` are defined further up the file, and most of them
again in other files:

```rust
#[test]
fn a_draft_pr_with_a_pass_is_marked_ready_once_then_merged() {
    let c = setup("drive-839-draft");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let ready = "gh pr ready 100 -R jirathip-dev/sendmeter";
    let kinds = |k: &str| read(&c.state).unwrap().iter().filter(|e| e.kind == k).count();
    let r = fake(NONE)
        .reply("gh pr view issue-12", &draft_pr(&head, "green"))
        .reply("gh pr ready 100", "")
        // three more scripted replies: the merge call, the merge commit, the issue close
        .reply("gh issue", "");
    let (out, _) = drive(&c, &r);
    assert_eq!(r.count(ready), 1, "{out}");
    assert_eq!(kinds("marked-ready"), 1);
    // then the same five replies again without the draft flag, a second drive, and four more asserts
}
```

On the builders, the whole test with every assert it has today:

```rust
#[test]
fn a_draft_pr_with_a_pass_is_marked_ready_once_then_merged() {
    let w = World::new("draft-ready")
        .issue(12, |i| i.impl_lane(Lane::gone("")).verdict(Pass, HEAD))
        .pr(12, Pr::open(HEAD).ci(Green).draft());
    let pass = w.drive();
    assert_eq!(w.count("gh pr ready 100"), 1, "{}", pass.out);
    assert!(!w.merged_a_pr(), "a draft never merges: {}", pass.out);
    assert_eq!(w.events("marked-ready").len(), 1);
    let w = w.pr(12, Pr::open(HEAD).ci(Green));
    let pass = w.drive();
    assert_eq!(pass.steps, ["merge"], "{}", pass.out);
    assert_eq!(w.count("gh pr ready 100"), 1, "marked ready once only: {}", pass.out);
    assert_eq!(w.state(12), "merged");
}
```

36 lines become 14 and the file needs none of the nine helpers. The test says which facts matter: a gone impl
lane, a PASS at the head, a green draft PR.

## A test that talks to serve

```rust
let board = w.client().paired();
let cards = board.screen("/screens/cards");
let res = board.action("cards", "ask-q7", "answer", json!({"choice": 1}));
assert_eq!(res.status, 200);
```

Over a real socket, when the socket is the point:

```rust
let live = Client::tcp(&addr).paired().sse("/api/v1/rows/stream");
assert!(live.next_data(Duration::from_secs(2)).is_some());
```

Every request closes its connection, so no test waits out serve's 5 s read timeout again.

## A lane runs the tests of what it touched

```
cargo test --test drive a_draft_pr          one area binary, one test
cargo test --test drive                      the area
cargo test                                   everything on the PR path: about 45 binaries
```

A file that still has its own binary is run by its own name, as today.

## The soak run

Three test targets hold the slow tests, each test ignored by default, so neither a lane's run nor the PR shards
start them. One nightly step runs them in release, with no list of test names:

```
cargo nextest run --release --run-ignored all -E 'binary(perf) | binary(soak) | binary(sim_soak)'
```

Until adoption that step is drover's own nightly workflow; adoption makes it gatekeeper's nightly gate, with the
same three targets named under `soak` in its config. A red run is a red scheduled workflow, as a red sim run is
today; after adoption drover files an issue for it (gatekeeper#99's slice D1). The PR path keeps a small check
of each moved thing: six sim tests, the pipeline replay over 150 events, `ask_scale` at 2,000 events, two
compaction seeds.

## The close

Slice 12's PR body ends the diet with the measured figures and what is left:

```
tests 55,940 / src 54,870 = 1.02; gatekeeper's ceiling rests at 1.05. On its slow list: 0.
Left for 1.00: the page-client tests (#1665, about 1,700 lines).
```

From then on the ceiling falls whenever a PR takes the ratio a step under it.
