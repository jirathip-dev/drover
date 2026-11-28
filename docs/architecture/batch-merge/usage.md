# batch-merge: the callers, first

## The owner

```toml
[fleet.drover]
batch_max = 4        # unset or 1: one PR at a time, today's rule
```

Nothing else changes for the owner. `drover merge <fleet> <issue>` stays the one command, by hand or from drive.
Its exit code and refusal speak for the issue it names, as today.

## `gate::merge` after the change

```rust
pub fn merge(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, issue: u64) -> Result<()> {
    let events = working(&ctx.state)?;
    if already_merged(out, &events, fleet, issue)? {
        return Ok(());
    }
    let lock = crate::drive::job::merging(ctx, cfg, r, fleet, issue)?;
    if recovered(ctx, cfg, r, out, &events, (fleet, issue))? {
        return Ok(());
    }
    let lead = match eligible(ctx, cfg, r, &events, (fleet, issue))? {
        Ok(ready) => ready,
        Err(rf) => return refuse_as_today(ctx, cfg, &events, (fleet, issue), rf),
    };
    let riders = batch::plan(ctx, cfg, r, out, &events, fleet, &lead)?;
    batch::run(ctx, cfg, r, out, (fleet, &lock), lead, riders)
}
```

- `eligible` is today's gates at one PR's head, lifted out of `merge` unchanged: PASS at the head, CI green, the
  serial mark, `Repro: verify`, the reroot. It returns the PR and its head, or today's refusal.
- `batch::plan` returns the riders: none when batching is off or the lead goes alone (it prints why).
- `batch::run` is the one path. With no riders it is today's merge: one premerge run, no retry, one
  `gh pr merge`, no batch event. `premerge` and the tail of `merge` move there; nothing is left twice.

## `batch::run`, read top to bottom

```rust
pub fn run(ctx, cfg, r, out, (fleet, lock): (&str, &Lock), lead: Ready, riders: Vec<Ready>) -> Result<()> {
    for _ in 0..2 {
        let stack = Stack::build(ctx, r, f, lead.clone(), &riders)?;
        let verdict = check(ctx, cfg, r, (fleet, lock), &stack)?;
        if stack.base_moved(ctx, r)? {
            continue;
        }
        return match verdict {
            Checked::Green => land(ctx, cfg, r, out, fleet, &stack, stack.len()),
            Checked::Culprit { at, check, output } => {
                land(ctx, cfg, r, out, fleet, &stack, at)?;
                charge(ctx, cfg, r, out, fleet, &stack, at, (check, &output))
            }
        };
    }
    refused("STAGING_MOVED", …)
}
```

- `Stack::build` merges each head onto the scratch worktree in order. The lead is the bottom: its conflict is
  today's `NOT_MERGEABLE`. A rider that conflicts is left out and nothing is logged on its issue.
- `check` runs the gate on the top. Red on a stack of two or more: once more, then a binary search over the
  prefixes. It renews the merge lease before each run and logs each run.
- `land(…, k)` merges the first `k` PRs. `charge` gives PR `at` today's `premerge_failed` and returns the
  refusal when `at` is the lead.

## What the log shows

A green batch of three (#12 led):

```
batch-planned  {batch: "9f2c1e…", base: "41059a8", members: [{issue: 12, pr: "1901", head: "…"}, {15…}, {19…}], dropped: [{issue: 21, why: "conflict"}]}
batch-checked  {batch: "9f2c1e…", tree: "9f2c1e…", upto: 3, green: true, secs: 412}
merged         #12 {pr, merge_sha, verdict_sha, into, premerge_secs: 412, batch: "9f2c1e…"}
merged         #15 {pr, merge_sha, verdict_sha, into, batch: "9f2c1e…"}
merged         #19 {pr, merge_sha, verdict_sha, into, batch: "9f2c1e…"}
batch-landed   {batch: "9f2c1e…", outcome: "landed", landed: [12, 15, 19], left: []}
```

One culprit among eight, sixth in the stack: `batch-checked` five times (`upto` 8 red, 8 red, 4 green, 6 red,
5 green), five `merged`, `premerge-failed` and a drive `verdict FAIL` on the sixth at its head,
`batch-landed {outcome: "charged", culprit: <sixth>, landed: [five], left: [two]}`. The two left have no event
of their own; the next merge step leads them.

## What a lane and the board see

| Who | Sees |
|---|---|
| A PR that landed in a batch | what it sees today: its PR merged, its issue closed with the evidence comment |
| The culprit | today's premerge FAIL at its head, naming the PRs it was stacked on; an uncounted fix round |
| A rider left out or left over | nothing; it is still `ready`, and its merge step says `IN_FLIGHT … one merge at a time` while a batch runs |
| `drover stats land <fleet>` | one more line: batches, median size, premerge runs per merged PR |

## The tests (`crates/cli/tests/batch.rs`), named for what a user sees

```rust
#[test]
fn a_green_batch_runs_the_gate_once_and_merges_each_pr_at_its_own_head() {
    let (c, r) = fleet("batch_max = 4").ready(&[12, 15, 19]).gate(&["ok"]);
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("sh -c"), 1);
    assert_eq!(r.calls("gh pr merge"), ["1912 … --match-head-commit h12", "1915 … h15", "1919 … h19"]);
    assert_eq!(merged(&c), [12, 15, 19]);
}

#[test] fn a_red_stack_green_on_its_retry_lands_whole_and_is_logged_flaky() {}
#[test] fn a_bisect_charges_the_one_culprit_among_eight_in_five_runs_and_lands_those_under_it() {}
#[test] fn a_serial_pr_and_a_migration_pr_each_merge_alone() {}
#[test] fn a_rider_that_conflicts_is_left_ready_with_no_event() {}
#[test] fn a_head_that_moved_stops_the_landing_and_the_rest_resume_from_the_logged_runs() {}
#[test] fn with_batch_max_unset_merge_logs_no_batch_event() {}
```

The first four are the issue's AC. Every existing test in `crates/cli/tests/merge.rs` passes unedited after
slice 1: that is the proof that a stack of one is today's merge.
