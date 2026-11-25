# test diet: the data shapes (#1760)

Sketches for the impl lanes and the reviewer. Names may change in review; the shapes may not without a note in
the PR body. `usage.md` shows each one in use.

## 1. One step of the ratchet

A file in the folder `test-cap.d`, beside `cap.d` under `ci`. One file per step, named by date and value.

```toml
# ci/test-cap.d/2026-11-26-148.toml
test_pct = 148
why = "#N: the cap starts at today's ratio: tests 94,937 / src 64,744 = 146.6%, rounded up, plus one"
```

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    test_pct: i64, // 1..=1000; a percent of src lines
    why: String,   // the issue and the measured figures
}

/// The test cap in force: the lowest step. No folder or no file: no test cap, and the check prints nothing.
pub struct TestCap {
    pub pct: i64,
    pub from: String, // the file that set it
}
```

Rules of the folder:

- the lowest `test_pct` wins, whatever the file names say;
- a file that does not parse fails the check (exit 1): a broken step is never read as "no cap";
- the folder is `append_only` in `.drover/arch.toml`: a PR may add a file, never change or remove one. So
  nothing a PR can add raises the cap.

## 2. What is counted

```rust
fn src_globs() -> Vec<String>  { vec!["src/**/*.rs".into(), "crates/*/src/**/*.rs".into()] }
fn test_globs() -> Vec<String> { vec!["tests/**/*.rs".into(), "crates/*/tests/**/*.rs".into()] }
```

Every line of every matching file, blank lines and all, as the line cap counts by default. The src figure of the
test cap is always the built-in globs: it does not follow `count` and `exclude` in `ci/cap.toml`, so a crate that
leaves the line cap stays in the ratio with its tests. Goldens, scripts and other non-Rust files under a `tests`
folder are not counted.

The rule: `tests * 100 <= src * pct`.

## 3. What the cap check prints

After its two lines of today, one more, and a second when a step set the cap:

```
src lines (formatted): 64744 / cap 65963
cap raised by ci/cap.d/2026-11-24-65963.toml
test lines: 94937 / cap 95821 (148% of 64744 src)
test cap from ci/test-cap.d/2026-11-26-148.toml
web lines (TypeScript, soft budget, not capped): 1990
```

Exit 1 when tests are over the cap; the same line is the finding. No `warn_within` for tests: the next step of
the ratchet is the warning.

The finding for a changed or removed step comes from arch-check, not from the cap check:

```
ci/test-cap.d/2026-11-26-148.toml:1: `test_pct = 148` removed or changed in an append-only file
```

## 4. The ledger in a PR body

A section of the PR body of any PR that removes a test. One line per removed test function.

```
## Tests removed

- `drive_sim::a_passed_draft_pr_is_marked_ready_and_merged` → kept by `drive::a_draft_pr_with_a_pass_is_marked_ready_once_then_merged` (dup)
- `merge::refuses_without_verdict` → row of `merge::refusals` (table)
- `term_adapter::only_term_rs_builds_pane_words` → none (impl: greps src for two word lists; a ban says the same)
- `owner_extend::a_975_pending_extend_is_requeued_once_and_its_card_never_comes_back` → none (dead: the one-off requeue of #1058)

Uncovered on purpose:
- `crates/core/src/pending.rs` 212-230: `requeue_waiting`, deleted in this PR with its test
```

```
line    = "- `" test "` → " target " (" reason ")"
test    = file-stem "::" fn-name                 ; as at the base
target  = "kept by `" test "`"                   ; reason dup: the keeper exists at the head
        | "row of `" test "`"                    ; reason table: the old fn name is a string in that test's file
        | "none"                                 ; reason impl or dead, with why after a colon
reason  = "dup" | "table" | "impl: " text | "dead: " text
```

For each keeper of a `dup`, once per keeper:

```
Kept-by: run `drive::a_draft_pr_with_a_pass_is_marked_ready_once_then_merged`
Change: crates/core/src/drive.rs: the `gh pr ready` call skipped
```

followed by the fenced command and its fenced failing output, as a `Repro: run` block is written.

## 5. What the keep job prints

```
tests removed: 14, in the ledger: 14
keepers and rows found at the head: 14 of 14
src lines covered at the base 31,204, lost at the head: 0
```

and, failing:

```
not in the ledger: shed::a_wait_that_times_out_answers_as_herdr
no such keeper at the head: spawn::an_acp_session_never_ready_gets_no_prompt
lost cover: crates/core/src/term.rs 88-93 (covered at the base by: shed::a_wait_that_times_out_answers_as_herdr)
```

A removed test is a `#[test]` function name present at the base and absent at the head, by bare name with its
count: a test that moves to another file or module is not removed.

## 6. The builders

In the cli crate's `tests/common`. Signatures only; slice 6 writes them and may adjust.

```rust
/// A fleet on a calm main host with a fake shed: what 63 `setup`s and 35 `fake`s build by hand.
pub struct World { /* ctx, FakeShed, Fake with the host's replies, the clock */ }

impl World {
    pub fn new(name: &str) -> World;
    pub fn config(self, edit: impl FnOnce(&mut Cfg)) -> World;      // typed edits: limit, role, fleet key
    pub fn host(self, h: Host) -> World;                             // Host::calm() is the default; busy, mem_low, disk
    pub fn issue(self, n: u64, facts: impl FnOnce(Issue) -> Issue) -> World;
    pub fn pr(self, n: u64, pr: Pr) -> World;
    pub fn at(&self, secs: u64) -> World;                            // the same world, later

    pub fn run(&self, args: &[&str]) -> (i32, String);               // the CLI entry, in process
    pub fn drive(&self) -> Pass;                                     // one pass; exit 0 asserted
    pub fn events(&self, kind: &str) -> Vec<Event>;
    pub fn state(&self, n: u64) -> String;
    pub fn sent(&self, lane: &str) -> Vec<String>;                   // what was typed into a lane
    pub fn called(&self, prefix: &str) -> bool;                      // a command the runner saw
    pub fn count(&self, prefix: &str) -> usize;
    pub fn merged_a_pr(&self) -> bool;                               // the merge call reached the runner
    pub fn client(&self) -> Client;                                  // the board, in process
}

pub struct Issue;   // .queued(&["--design"]) .impl_lane(Lane::idle()) .pushed(head) .rev(1)
                    // .verdict(Pass, head) .fail(head, body) .needs_human(code) .merged(pr)
pub struct Lane;    // Lane::idle() | working() | gone(cause) | hibernated()
pub struct Pr;      // Pr::open(head).ci(Green | Red(log) | Pending).draft().conflicting().files(&[..])
pub struct Pass { pub out: String, pub steps: Vec<String> }

/// One HTTP client. `inproc` calls the server's handler; `tcp` opens a socket and always sends `Connection: close`.
pub struct Client;
impl Client {
    pub fn inproc(s: &Server) -> Client;
    pub fn tcp(addr: &str) -> Client;
    pub fn paired(self) -> Client;                                   // a paired phone's cookie
    pub fn get(&self, path: &str) -> Res;
    pub fn post(&self, path: &str, body: &str) -> Res;
    pub fn screen(&self, path: &str) -> Value;
    pub fn action(&self, screen: &str, block: &str, action: &str, values: Value) -> Res;
    pub fn sse(&self, path: &str) -> Stream;                         // .next_data(deadline)
}
pub struct Res { pub status: u16, pub head: String, pub body: String } // .json()

/// A bare origin, a clone with a first commit pushed, and a runner that lets `git` through to the real one.
pub struct Repo;    // Repo::new(name) .branch("issue-12") .commit(file, text) .worktree(12, "impl") .git(&[..])

/// One generator of a large log: what seven hand-written ones build.
pub struct Log;     // Log::new(&ctx).fleets(5).issues(40).asks().done(150).fill(150_000).write()

/// The lane hook, run with a tool's input.
pub struct Hook;    // Hook::lane("tangelo:12:impl").bash(cmd) -> Allow | Deny(reason); rows(&h, want, &[cmds])
```

What a builder must not do: assert. It builds a world and reads it back; the test owns every `assert`.

## 7. A table test

```rust
#[test]
fn refusals() {
    let rows: &[(&str, fn(World) -> World, &str)] = &[
        ("refuses_without_verdict", |w| w, "NO_VERDICT"),
        ("refuses_verdict_for_older_sha", |w| w.issue(12, |i| i.verdict(Pass, OLD)), "STALE_VERDICT"),
        ("refuses_fail_verdict", |w| w.issue(12, |i| i.fail(HEAD, "x")), "VERDICT_FAIL"),
    ];
    for (name, arrange, code) in rows {
        let w = arrange(World::new(name).pr(12, Pr::open(HEAD).ci(Green)));
        let (exit, out) = w.run(&["merge", FLEET, "12"]);
        assert!(exit == 3 && out.contains(code), "{name}: {out}");
    }
}
```

A row's first field is the old test's name. It names the temp dir, it is what a failure prints, and it is the
text the keep job looks for.

## 8. Where test files live

```
crates/cli/tests/
  common/            the builders and the fakes; compiled once per binary
  drive/main.rs      mod pass; mod kicks; mod turn; mod waiting; ...      one binary: `--test drive`
  drive/pass.rs      today's drive.rs
  gates/  lanes/  guard/  host/  ops/  asks/  serve/  screens/  sim/  perf/
  turn.rs            a file that sets process environment keeps its own binary
```

A module inside any binary may hold a `nightly` module:

```rust
mod nightly {
    #[test]
    #[ignore = "nightly"]
    fn the_hunt_finds_a_planted_bug_sooner_than_random_seeds_and_minimises_it() { /* ... */ }
}
```

A budget that needs a release build: `#[cfg_attr(debug_assertions, ignore = "release only")]`, in place of an
early return in debug.
