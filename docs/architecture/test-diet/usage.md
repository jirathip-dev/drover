# test diet: how the callers read (#1760)

Sketched before anything is built. Each block is what someone sees or writes once the slices have landed.

## A lane whose PR brings too many tests

CI's line-cap step, and the same check at merge on the PR merged with the integration branch:

```
src lines (formatted): 55310 / cap 56000
test lines: 71903 / cap 71349 (129% of 55310 src)
test cap from ci/test-cap.d/2026-12-04-129.toml
exit 1
```

The lane reads one line and has two moves: add the case as a row of the table that already tests the verb, or
delete a test its change made redundant (with its ledger line). It never asks for a raise: there is none.
`roles/impl.md` says so in one sentence beside the line-cap rule.

## A diet slice lowers the ratchet

The last commit of the slice measures and adds one file:

```toml
# ci/test-cap.d/2026-12-04-129.toml
test_pct = 129
why = "#N slice 5: tests 70,410 / src 55,310 = 127.3%, rounded up, plus one"
```

Nothing else is edited. Two slices that land the same day add two files; the lower one wins.

## Someone tries to raise it

Editing a step, or deleting one, fails arch-check on that PR and at `drover turn done`:

```
ci/test-cap.d/2026-12-04-129.toml:1: `test_pct = 129` removed or changed in an append-only file
```

Adding a file with a higher number changes nothing: the lowest wins.

## A PR that deletes tests

Its body carries the ledger (`shapes.md`, section 4):

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

## The nightly run

One step, no list of names:

```
cargo nextest run --release --run-ignored ignored-only -E 'test(/nightly::/)'
```

A red nightly files its issue the way the sim workflow does today. The PR path keeps a small check of each
moved thing: six sim tests (about 128 s), the pipeline replay over 150 events, `ask_scale` at 2,000 events, two
compaction seeds.

## The close

Slice 12's PR body ends the diet with the measured figures and what is left:

```
tests 55,940 / src 54,870 = 102%; the ratchet rests at 103.
Left for 100%: the page-client tests (#1665, about 1,700 lines).
```

From then on the ratchet falls only when someone adds a lower step.
