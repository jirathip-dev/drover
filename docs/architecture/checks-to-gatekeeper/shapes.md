# The 4 checks in gatekeeper: data shapes (#1643)

Revised 2026-11-26: the cap check, its gate file and `ci/cap.toml` are out (#1812; `adr.md`, decision 13).

## What a check is given (gatekeeper)

A built-in check keeps today's signature: typed values in, finding lines out. Three values are new.

```rust
// crates/gatekeeper-model
/// The fleet's private names for one run. Built only from a list with a name in it.
/// No Debug, Display or Serialize: a name cannot reach a log, a report or a cache key by accident.
pub struct PrivateNames(Vec<String>);

impl PrivateNames {
    /// One name a line, trimmed; `None` when no name is left.
    pub fn parse(text: &str) -> Option<PrivateNames>;
    /// The 1-based number of the first name in the list `line` holds as a whole word, any case.
    /// The shared names set (below) fixes what that means.
    pub fn hit(&self, line: &str) -> Option<usize>;
    pub fn len(&self) -> usize;
}

// crates/gatekeeper-engine/src/run/cx.rs
pub struct RunCtx {
    // … today's fields, `base` and `pr_body` among them …
    pub names: Option<PrivateNames>,
    pub head: Option<String>,
}

pub struct CheckCx<'a> {
    // … today's fields, `root`, `scratch` and `base` among them …
    pub names: Option<&'a PrivateNames>, // hygiene
    pub head: Option<&'a str>,           // pr-body; None: `git rev-parse HEAD`
    pub pr_body: Option<&'a Path>,       // pr-body, hygiene, arch (`no doc:`)
    pub integration: &'a str,            // pr-body: the walk under merges of this branch
}

// crates/gatekeeper-engine/src/check/<name>.rs, one file a check: rules, arch, hygiene, pr_body
pub fn run(cx: &CheckCx) -> Result<Vec<String>>;
```

Who fills the new values:

| Value | In CI and on a lane's host | In a fixture case |
|---|---|---|
| `names` | the CLI reads `DROVER_PRIVATE_NAMES` once | the case's `env` file, as today; or its `names` file, one name a line (new, G3: an `env` line holds one name only) |
| `head` | the CLI reads `GATEKEEPER_HEAD` once | the case's commit |
| `pr_body` | `--pr-body <file>`, as today | the case's `body` file, as today |

The engine reads no environment itself: that ban is in gatekeeper's `arch.toml` already.

## What each check prints

Unchanged. The fixture `expect` files hold these lines today and are not edited by the swap.

| Check | A finding line |
|---|---|
| `rules` | `FAIL <name>: <cmd>` with its output; `line 7 names no enforcer: …`; `31 lines, over 30: …` |
| `arch` | `<file>:<line>: <why>` for a layer, ban, size, frozen, generated, shared, comment or doc finding |
| `hygiene` | `private-name #2 at <file>:<line>`, `… at commit <sha> message`, `… at PR body:<line>`; `list-missing: …` |
| `pr-body` | `PR_BODY: the PR body has no ## For humans section: …`, and the two other reasons |

A passing line that is not a finding (a shadow check that would fail, the number of names) goes
to the gate's note and log, never to the findings.

## The gate files after

```toml
# packs/core/gates/hygiene.toml
about = "the tree, the PR's commit messages and its body against the private-names list"
fix = "take the name out where the hit points; CI passes the private-names secret, a private repo turns this gate off with a reason"
budget_secs = 5
needs = ["private-names"]
cache = false

[[step]]
check = "hygiene"
```

`rules.toml`, `arch.toml` and `pr-body.toml` change the same way: the step becomes `check = "<name>"`, the budget
stays. `hygiene` and `pr-body` set `cache = false`, since the list and the PR body are not files of the tree.

## The rules report

The one new contract between the repos. The `rules` check writes it on every run, pass or fail:

```
target/gatekeeper/check/rules/checks.json
```

```rust
pub struct RulesReport {
    pub file: String,            // ".drover/rules.md"
    pub checks: Vec<LessonCheck>,
}

pub struct LessonCheck {
    pub line: u32,               // 1-based line in the rules file
    pub name: String,
    pub cmd: String,
    pub state: CheckState,
    pub since: String,           // YYYY-MM-DD
    pub passed: bool,
}

#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Shadow,
    Active,
}
```

```json
{"file": ".drover/rules.md",
 "checks": [{"line": 21, "name": "no-todo", "cmd": "sh scripts/no-todo.sh",
             "state": "shadow", "since": "2026-11-20", "passed": false}]}
```

Drover reads it with its own struct of the same field names and ignores fields it does not know. A hand-written
rule line (no `since` mark) is not in the list: the lesson lane never runs or changes one.

## The repo files: same paths, one owner each

| File | Its format is owned by | Who else reads it |
|---|---|---|
| `.drover/rules.md` | gatekeeper's `rules` check | drover: whole, into every brief; the lesson lane rewrites lines by number |
| `.drover/arch.toml` | gatekeeper's `arch` check | drover: `frozen`, `generated`, `shared`, for hotspots |
| `.drover/arch-baseline.txt` | gatekeeper's `arch` check | nobody |
| the names list | drover's fleet config (`private_names`) | gatekeeper, only as `DROVER_PRIVATE_NAMES` |

The arch schema is today's, less one key (`[docs] cli`).

## Drover's one gatekeeper module

```rust
// crates/core/src/gatekeeper.rs: the only file that names the wrapper or a file gatekeeper writes
/// The wrapper's check as a lane runs it, and the done check's name.
pub const CHECK: &str = "./.gatekeeper/run check";

pub struct Pr<'a> {
    pub head: &'a str,
    pub body: &'a str,
}

/// The wrapper's path in the repo at `dir`.
pub fn wrapper(dir: &Path) -> PathBuf;
/// Write `pr`'s body into `wt` on `on`'s host and run the wrapper once there, `names` on stdin.
pub fn check(on: &On, wt: &Path, pr: &Pr, names: &[String], limit: Duration) -> Result<Out>;
/// Run `core/rules` in `wt` and read its report; `None` when the pinned release writes none.
pub fn rules(on: &On, wt: &Path) -> Result<Option<RulesReport>>;
/// Rewrite a gate's baseline (setup's step for `core/arch`).
pub fn baseline(on: &On, wt: &Path, gate: &str) -> Result<Out>;
```

`CHECK`, `Pr`, `wrapper` and `check` are on staging as D3 merged them. `rules` arrives with D4 and `baseline`
with D5. D3 also added `cap(dir)`, the merge check's cap command; #1812 deletes it. `Out` is the runner's exit
code and output; `done.rs` turns it into the refusal text.

## The shared names set (G3, D2)

Private names are matched by gatekeeper's `PrivateNames::hit` and by the guard's own function in drover. Both
must give this set's `expect` lines. It is one fixture case of `core/hygiene`, three files, the same bytes in both
repos:

| File | What | Rules |
|---|---|---|
| `names` | the list, one name a line | made-up names only; no blank line, so a name's number is its line |
| `lines.md` | the text, one case a line | no home path and no UUID in it: the guard names those first |
| `expect` | `private-name #<n> at lines.md:<line>` for each line that hits, in line order | a line that is not listed must not hit |

```
# names                # lines.md                                   # expect
quillmoor              Ship it to the Quillmoor team.               private-name #1 at lines.md:1
north-pier             quillmoors and requillmoor2 are other words. private-name #1 at lines.md:3
pier nine              See quillmoor_notes and v2-quillmoor/x.      private-name #2 at lines.md:4
quill                  The north-pier build.                        private-name #3 at lines.md:5
                       Meet at Pier Nine.                           private-name #1 at lines.md:6
                       A quill, then quillmoor.                     private-name #4 at lines.md:7
                       A quill alone.
                       équillmoor is another word.
                       Nothing here.
```

The sample is today's behaviour: `drover hygiene check` over these nine lines with these four names prints exactly
the six `expect` lines (run 2026-11-23). What the set fixes, each with a line of its own:

- any case hits; a name inside a longer word does not, and a letter or digit of any script makes a longer word;
- every other neighbour is an edge: a space, the line's start or end, punctuation, `_`, `-`, `/`;
- a name may hold a space, a dot or a hyphen;
- with two names on a line, the lower number is reported, not the one further left;
- a line gives one hit at most.

Where it lives, and who checks it:

| | Path | Checked by |
|---|---|---|
| gatekeeper (the home, from G3) | `packs/core/fixtures/hygiene/shared-names/` | selftest: the gate fails on the case and prints `expect` |
| drover (from D2) | the `private-names` folder under the CLI crate's test goldens (the command below names it) | one test in `tests/hygiene.rs`: the guard over each line gives `expect` |

Gatekeeper wrote the set in G3, from this sample: D1 merged without drover's copy. Drover's copy arrives in D2 and
is a generated file from its first day: `just gen` writes it from the pinned release with the `fixture` verb:

```
$ ./.gatekeeper/run fixture core/hygiene/shared-names --to crates/cli/tests/golden/private-names
wrote names, lines.md, expect
```

`fixture <pack>/<gate>/<case> --to <dir>` writes the case's files as the pack holds them, its `expect` and other
case files included, and leaves nothing else in `<dir>`.

## The pre-send guard's check (#1615, slice D1, merged)

```rust
pub struct Check {
    // … today's fields: repo, hosts, private, dirs, private_repos …
    /// The session ids drover or shed knows when the check is built. A UUID outside this list is no `session-id`.
    #[serde(default)]
    pub sessions: Vec<String>,
}
```

In the hook, an id that names an entry of the lane host's session folders counts as known too. A UUID that is in
the integration tree already never hits. Both are #1615's own rules.
