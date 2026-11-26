# ADR: the 4 checks (rules, arch, hygiene, pr-body) move into gatekeeper's core pack; drover runs them through gatekeeper (#1643)

## Summary

Decision: gatekeeper owns four checks as its own code: rules, arch, hygiene and PR body. The line cap is no longer one of them: the owner removed it (#1812). Gatekeeper deletes the cap check it already built (gatekeeper#127), and each repo drops its cap files in the same PR that bumps its gatekeeper pin. Everything else stands as approved.
Slices: done: gatekeeper's six, the session-id fix (#1615), the done gate through gatekeeper. Left: one PR per repo (pin 1.5.0 or later, cap files out), drover's own adoption (#1587), the lesson lane reading gatekeeper's report, last the four verbs deleted.
Open questions: (1) #1587 is held until #1643 merges, but #1643's own PR is the last slice and waits for #1587: swap that block? (2) After a repo's bump, a lane's own check in its pane fails the names gate, as the pane has no names list (#1642 was closed): leave it, since the done gate decides?

Status: approved 2026-11-23 (q3305). Revised 2026-11-26 on the owner's amendment (q3842): the cap check is removed
(#1812), so 5 checks become 4. This revision also records what has merged since. Read `README.md` first.

## Question

Gatekeeper's core pack ran five of its gates by calling a pinned drover binary. Where does the code of those
checks live, what may it read, how does drover's done gate run it on a host that has none of drover's config, and
in which order do two repos and their consumers change so that no repo is checked less on any day? Since
2026-11-26 also: one of the five, the line cap, is removed everywhere. How does it leave without a red day?

## Where it stood (2026-11-23: drover staging 7c0a28c1c, gatekeeper main aa132f9)

| Gate | Drover verb it called | The code in drover | Lines that move |
|---|---|---|---|
| `core/rules` | `lesson-check` | part of `lesson.rs` | about 75 |
| `core/arch` | `arch-check` | `arch.rs` | about 690 of 703 |
| `core/hygiene` | `hygiene check` | part of `hygiene.rs` | about 95 of 527 |
| `core/pr-body` | `pr-body` | part of `done.rs` | about 75 |

The fifth, `core/cap` (`cap-check`, about 150 lines of `overlap.rs`), is out of this work: see decision 13.

- The pack pinned one drover build in its tool file. The binary is 17.6 MB per target. A fix to a check was a
  drover release, a gatekeeper release and a bump per repo.
- Gatekeeper's boundaries doc had the rule "A pack calls a drover concept through drover. gatekeeper never
  re-implements one." The owner's decision of 2026-11-23 reversed it.
- `hygiene check` read the fleet's `private_names` from the fleet config on the host it ran on. A remote host has
  none, so the gate failed `list-missing` there (#1642).
- The PR body rule was written three times: `turn done`'s own check (refusal `PR_BODY`), the `pr-body` verb, and
  the shell script behind the `evidence` workflow.
- Drover's adoption PR (#1634 for #1587) built drover before it could check itself: a `[tools]` line, a wrapper
  script and a 600 s budget for `core/arch`.
- `arch-check`'s `[docs] cli = true` checks docs against drover's own command tree. Only drover's repo sets it.
- Only part of `hygiene.rs` is the gate. The rest is the pre-send guard (`PUBLIC_LEAK`), `hygiene scan` and
  `hygiene push`. #1615's `session-id` pattern is in the guard, not in the gate.
- The guard and the gate found a private name with one function: a whole word, any case, reported by its number
  in the list. The guard needs it before every send, with no repo wrapper at hand, so after the move the rule is
  written in both repos.
- Drover's merge ran the shadow lesson checks itself to record their history; the lesson lane promotes or retires
  by that history.
- A built-in check in gatekeeper is one file in a collected folder with a generated module list. Every gate has
  fixture cases with the text each must print. Gatekeeper's files are capped at 400 lines, its functions at 60.

## Where it stands (2026-11-26: drover staging 53d0dd927, gatekeeper staging at 1.5.0)

- Gatekeeper's six slices are merged. No 1.4.0 was released: the built-in checks first shipped in **1.5.0**
  (2026-11-25). `git grep` finds no drover verb in its `packs/` at v1.5.0. `core/cap` shipped in it as a built-in
  check.
- In drover, #1615 (D1) and the done gate (D3) are merged. The five verbs are still there. The shared names set is
  in gatekeeper only: D1 merged without drover's copy and its test.
- No repo pins 1.5.0 yet (integration branches, read today): seven repos pin 1.0.0 to 1.1.2, one pins 1.3.0. Every
  one of those releases calls a pinned drover build for the checks.
- #1812 (queued) removes the cap from drover. Each repo with a cap has its own removal issue, blocked on #1812:
  gatekeeper#127, cooee#834, shed#392, blocks#414, sendmeter#1067.
- At every release so far, 1.0.0 to 1.5.0, `core/cap` fails when `ci/cap.toml` is missing. A gate can be turned
  off with a reason from 1.2.0 on.

## Options

### 1. Where the code lives

| Option | Verdict |
|---|---|
| A. Stay: a pinned drover, and ship the config it needs to every host (#1642 alone) | No. Every job still fetches drover, every fix still needs two releases, and drover's own repo still builds itself first. The owner decided against it. |
| B. Split a small checks binary out of drover and pin that | No. A smaller download, the same two-release path, and the checks are still drover's. |
| C. Gatekeeper links a crate kept in drover's repo | No. One copy, but gatekeeper's build then hangs on drover's repo and its libraries. Gatekeeper links no other repo's code today. |
| D. Port: built-in checks in gatekeeper, printing what they print today | **Yes.** |

### 2. The files the checks read

| Option | Verdict |
|---|---|
| A. Keep the paths: `.drover/rules.md`, `.drover/arch.toml`, the arch baseline | **Yes.** Drover still reads both (briefs, hotspots). A check ported with its files in place is proven by fixtures nobody edits. |
| B. Move them under gatekeeper's folder | No. Every repo, every role prompt and hotspots change for no change in behaviour. It can be its own issue later. |

### 3. Where the private names come from

| Option | Verdict |
|---|---|
| A. One env var, `DROVER_PRIVATE_NAMES`, set by CI from the secret and by drover on a lane's host | **Yes.** Releases before and after the move read the same variable, so nothing sets two. |
| B. Rename the variable for gatekeeper | No. During the bumps drover and the workflow would have to set both names. |
| C. A names file on each lane's host | No. A secret list at rest on every host. |
| D. Skip the gate on a lane's host when no list is there | No. A green local check must mean a green CI, and #1642 asks for the same result on any host. |

### 4. How `turn done` runs the checks

| Option | Verdict |
|---|---|
| A. One wrapper run on the lane's host; drover hands it the PR body and the names; drover holds no check code | **Yes.** |
| B. Drover keeps its own PR body check beside gatekeeper's | No. Two copies of one rule. |
| C. The lane's host runs a drover verb that calls gatekeeper | No. It needs drover and its config on the host. |

### 5. Lesson checks: who runs them, who promotes them

| Option | Verdict |
|---|---|
| A. Gatekeeper runs every lesson check and writes a report; drover reads it to record, promote and retire | **Yes.** One parser and one runner. |
| B. Drover keeps a parser and runner for the shadow history | No. A check could be promoted on a run that differs from the one that enforces it. |
| C. Promotion moves too | No. It reads the event log, verdicts and thread lines, and the issue keeps it with the lesson lane. |

### 6. Private names are matched in two places

| Option | Verdict |
|---|---|
| A. One copy: the guard calls gatekeeper for each text it is about to send | No. The guard checks a piece of text before every send (the hook, a push, drover's own issue and PR text), in repos with or without the wrapper. Each call would start a process and hand it the names. |
| B. One copy, linked from the other repo | No. Option 1C again, in either direction. |
| C. Two copies and one shared set of cases that both must pass | **Yes.** The owner's revise (q3300). |
| D. Two copies, each with its own tests | No. Nothing fails when one of them changes. |

### 7. How a repo's `core/cap` leaves (new, 2026-11-26)

A repo on gatekeeper cannot just delete `ci/cap.toml`: its pinned `core/cap` then fails.

| Option | Verdict |
|---|---|
| A. Wait for the gatekeeper release without `core/cap` (gatekeeper#127), then each repo pins it and deletes its cap files | No. Every repo's cap stays on until gatekeeper's next release, with drover no longer raising it. |
| B. Turn the gate off at the repo's current pin and delete the files | No. The off list needs 1.2.0 and all but one repo pins an older release, so it is a bump anyway, and a second bump follows for this work. |
| C. One PR per repo: pin the newest release (1.5.0 or later), delete the cap files, turn `core/cap` off while that release still has it | **Yes.** The bump this work needs anyway carries the cap out. No repo waits for gatekeeper. |

## Decision

1. **Port, not link.** Each check is one file in gatekeeper's check folder; arch's parts get a folder of their own
   under the file limit. A check takes typed values and returns finding lines, as every built-in check does.
2. **Same findings.** The PR that swaps a gate from the drover tool to the built-in check may not edit that gate's
   fixture `expect` files. Drover's tests of the check arrive as new fixture cases. For arch, the PR also runs old
   and new on every consumer's integration head and shows equal finding lists.
3. **What moves and what stays** is the table below.
4. **Paths stay** (option 2A). One key leaves the arch schema: `[docs] cli`, which becomes a test in drover.
5. **Inputs.** A check reads repo files, git, and three values the gatekeeper CLI reads once: the names
   (`DROVER_PRIVATE_NAMES`), the PR's head (`GATEKEEPER_HEAD`) and the PR body (`--pr-body`). No check reads the
   fleet config, `DROVER_LANE` or the GitHub API.
6. **Private repos.** CI skips `core/hygiene` there, as today. For lanes, a private repo turns the gate off in its
   gatekeeper config with a reason. The plan refuses that line in a public repo's CI run, so it cannot outlive a
   change of visibility.
7. **The done gate** (option 4A). Drover writes the PR body to the lane's host and runs the wrapper once, the names
   on stdin. Its own PR body check goes; a bad body is a `DONE_CHECK` whose text names `core/pr-body` and its fix.
   The fallbacks go too: a repo with no wrapper and no `done_checks` is refused, and `done_checks = []` says "none"
   on purpose.
8. **Withdrawn 2026-11-26.** The merge check ran `core/cap` through the wrapper, and D3 built that. #1812 removes
   the cap from the merge check, and that call with it.
9. **Lessons** (option 5A). The `rules` check runs shadow and active checks and writes `checks.json`. Drover's
   merge reads it for the `lesson-shadow` event; `lesson apply` reads it for the lines to rewrite. Drover knows the
   two marks it writes, `shadow since` and `active since`, and no more of the line format.
10. **The arch baseline** is rewritten by gatekeeper's `baseline` verb and follows gatekeeper's rule: after the
    first write it only shrinks.
11. **One module in drover names gatekeeper.** It builds the wrapper's command line and reads its report. An arch
    ban keeps the wrapper's path out of every other file.
12. **One shared names set** (option 6C). A names list, lines of text and the hit each line must give
    (`shapes.md`). Gatekeeper holds it as a fixture case of `core/hygiene`, so its selftest runs it, and is its
    home. Drover's copy is a generated file, written from the pinned release, and one test runs the guard over it:
    a copy that differs fails `core/arch`. Both arrive in D2, since D1 merged without them.
13. **The cap check leaves** (owner, #1812; option 7C). It is not one of the checks this work moves, and no slice
    here builds or deletes anything for it. What exists is removed by the cap's own issues:
    - drover, #1812: the `cap-check` verb, the cap view, the hook's rule, the merge check's cap command and the
      module function D3 added for it.
    - gatekeeper, gatekeeper#127: the check, its gate file and its seven cases. An `[off]` line that still names
      `core/cap` keeps loading (its spec says so).
    - each repo on gatekeeper, in its cap-removal issue: one PR pins the newest release, 1.5.0 or later, deletes
      the cap files and, while the pinned release still has `core/cap`, turns it off with the reason. That PR is
      also this work's bump for the repo.

| | Moves to gatekeeper | Stays in drover |
|---|---|---|
| rules | the line format, the 30-line limit, running each lesson check | the lesson lane: digest, the history, promote and retire, the `lesson-shadow` event |
| arch | all of it: layers, bans, sizes, frozen, generated, shared, comments, stale docs, the baseline | hotspots reading three list keys; the docs-against-CLI test |
| hygiene | the gate: tree, commit messages and PR body against the names; the shared names set's home | the pre-send guard (`PUBLIC_LEAK`) with its own name matcher, `hygiene scan`, `hygiene push`, doctor's line |
| PR body | all of it: names the head, has `## For humans`, no "Generated with" line | nothing |
| cap | nothing: removed in both repos (decision 13) | nothing |

## Consequences

- A lane's host and a CI job fetch one binary. The drover tool file left the core pack, so a config that still
  names it is refused.
- A check fix ships with a gatekeeper release. Drover releases no longer gate the gates.
- Three things a lane sees change. (a) The `PR_BODY` refusal code is gone; the same fault is `DONE_CHECK` with
  gatekeeper's lines. (b) `turn done` in a repo with neither the wrapper nor `done_checks` is refused, where today
  it passes with whatever fallback applies. (c) The arch baseline cannot grow: a redo on the owner's ask deletes the
  file first.
- The merge runs the lesson checks on the merged tree through the wrapper, shadow and active in one run, since
  one run produces the report.
- Drover still parses one thing gatekeeper owns: three list keys of `.drover/arch.toml`, for hotspots.
- Arch's patterns are read as gatekeeper globs. Where a repo's pattern means something else there, the parity run
  shows it and that repo's bump PR rewrites the pattern.
- The rule for a whole-word name match is written twice. A change to it is a gatekeeper PR that edits the set and
  its matcher; drover's next bump PR regenerates its copy and its guard must pass it in that PR. An edit of drover's
  copy alone fails `core/arch`.
- Gatekeeper gained one verb, `fixture`, which writes a fixture case's files to a folder: drover's generator.
- Drover loses about 1,000 lines for the four checks and gains about 200 (the module, the session ids); about
  2,000 lines of tests leave with the checks. The cap's 150 lines and its view leave with #1812. Gatekeeper gained
  about 1,500 and gives back the cap check (177 lines, seven cases) with gatekeeper#127.
- Until a repo bumps, its pinned drover build keeps working, #1812 or not: an old pin calls the drover build it
  pinned, never the host's. The done gate's env var and `--pr-body` are read by old and new releases alike.
- A repo's cap ends with its own PR, not with #1812. In between, its pinned `core/cap` still counts lines and
  drover no longer asks for a raise, so each repo's cap-removal issue goes to the top of its queue when #1812
  merges.
- A repo's bump is a bigger step than planned: 1.5.0 also turns the docs gate on. A new gate that finds
  something is turned off in that PR with a reason and its own issue, as gatekeeper's README shows.
- An `[off]` line for `core/cap` is dead once the repo pins a release without the gate. It loads, and the repo's
  next bump deletes it.

## Slices

Gatekeeper:

| # | Issue | State | Note |
|---|---|---|---|
| G0 | cap: raise gatekeeper to 9,500 | merged (gatekeeper#78) | the cap itself goes with G6 |
| G1 | core checks, the frame: typed names, head, PR body, integration branch | merged (gatekeeper#73) | |
| G2 | `core/pr-body`, `core/cap`, `core/rules` as built-in checks | merged (gatekeeper#74) | G6 deletes `core/cap` again |
| G3 | `core/hygiene` as a built-in check, the shared names set, the `fixture` verb | merged (gatekeeper#75) | the set was written there, from this ADR's sample |
| G4 | `core/arch` as a built-in check, with `baseline core/arch` | merged (gatekeeper#76) | |
| G5 | the drover tool leaves the core pack; release | merged (gatekeeper#77) | released as 1.5.0 |
| G6 | gatekeeper#127: the cap check and gatekeeper's own cap files are removed | open, blocked on #1812 | its docs include gatekeeper's copy of this ADR: replace it with this revision |

Every repo on gatekeeper, one PR each; a repo with a cap waits for #1812 first. It is the repo's cap-removal
issue where one is filed (cooee#834, shed#392, blocks#414, sendmeter#1067); a repo without one gets one issue
for both:

- `./.gatekeeper/run pin <the newest release, 1.5.0 or later>`;
- the cap files deleted, and `[off]` for `core/cap` unless the pinned release has dropped the gate;
- a private repo adds the `[off]` line for `core/hygiene`.

The repos not on gatekeeper adopt at the newest release, in gatekeeper's adoption order.

Drover:

| # | Issue | State, blocked on | AC |
|---|---|---|---|
| D1 | #1615: `session-id` hits only a known session id | merged (#1693, 3e8d8dede) | its own AC. The shared names set did not land with it: see D2 |
| D2 | #1587, amended again: adopt at the newest release, 1.5.0 or later | open; blocked on nothing (in the queue: on #1643, see the open question) | #1587's AC, and: no `[tools]` line, no script that builds drover; `[docs] cli` is a test; the `evidence` workflow and its script are deleted; the shared names set is a generated file, written from the pin, and one test runs the guard over it and gets its `expect` file; no cap file and no cap gate (after #1812: the `[off]` line of every repo) |
| D3 | #1721: the done gate and the merge check run the wrapper, through one module | merged (#1725, c1683b70c) | the remote-host test is in. Its cap command goes with #1812 |
| D4 | the lesson lane reads gatekeeper's report (to file) | every repo's PR above | the merge's `lesson-shadow` event and `lesson apply` come from `checks.json`; drover runs no lesson check itself |
| D5 | #1643's own PR: the four verbs and their code are deleted | D2, D4, every repo's PR, the adoptions | `git grep` finds none of `lesson-check`, `arch-check`, `hygiene check`, `pr-body` in drover's code, workflows or roles; drover's own PR body check is gone and a bad body is `DONE_CHECK` naming `core/pr-body`; the remote-host test of D3 still passes; setup's baseline step calls the wrapper; the PR body names #1587 and #1615 with their commits |

D3 kept drover's own PR body check, and the old path for a repo without the wrapper. D5 removes both, once every
repo's pin has `core/pr-body`: so no repo is checked less on any day. D5 does not wait for #1812 and does not
touch the cap: whichever merges second finds the other's verbs gone already.

## #1587, #1615 and #1642

- **#1587** is slice D2 and closes with its own PR (amendment of 2026-11-24). Its amendment of 2026-11-23 says
  "adopt at 1.4.0" and "waits for gatekeeper's 1.4.0 release": that release is 1.5.0, and it is out. It needs one
  more amendment before its lane continues: pin the newest release, 1.5.0 or later; the shared names set is
  generated from the pin, with the guard's test over it; the cap files its spec keeps as "drover's own" follow
  #1812. Its spec keeps the `evidence` workflow; with `core/pr-body` in the gates that workflow is a second copy,
  so D2 deletes it and its PR body names the check to drop from branch protection.
- **#1615** was slice D1 and is merged. Its pattern is in the pre-send guard, which stays in drover.
- **#1642** and its root #1646 are closed, not planned. What this work owed them is merged: D3's done gate hands
  the names over itself and passes on a remote host with no fleet config (#1816 tracks one route where they do
  not arrive). The pane is not covered: see open question 2.

## Open question

1. In the queue #1587 is "blocked by #1643" and held until #1643 merges. That came from the symptoms rule of
   2026-11-23; the amendment of 2026-11-24 made #1587 a slice. #1643's own PR is D5, the last slice, and it waits
   for #1587: neither can go first as queued. Swap the block, so #1587 runs now and #1643 waits for #1587, D4
   and the repo PRs? (Recommended: yes.)

2. A built-in `core/hygiene` reads the names from the env var only. The done gate sets it; drover sets it in no
   lane's pane, since #1642 (the names in the lane's env file) was closed. So once a public repo pins 1.5.0, a lane's own
   `./.gatekeeper/run check` shows `core/hygiene` failing `list-missing` on every host, the main one included,
   while `turn done` passes. Leave it, and have D5's role text say that the done gate decides that gate
   (recommended: no names list at rest on any host)? Or reopen #1642's spawn change?

The first draft's question, gatekeeper's cap raise, is closed: the raise merged and the cap now leaves.

## `.drover/arch.toml` lines

Drover. D3's ban is in: the wrapper's path is named in drover's one gatekeeper module and nowhere else. D2 adds
the rest:

```toml
[docs]
paths = ["docs/**/*.md", "README.md"]
watch = ["crates/core/src/cli.rs", "crates/core/src/config.rs", "crates/server/src/api.rs"]
skip = [".drover/**", "roles/usage-tail.md", "scripts/test-shards.txt"]
```

That block is today's `[docs]` without `cli = true`. D2 also adds three paths to the `generated` list, and one
line to `just gen`, the `generate` command:

```toml
generated = [
    # … today's entries …
    "crates/cli/tests/golden/private-names/names",
    "crates/cli/tests/golden/private-names/lines.md",
    "crates/cli/tests/golden/private-names/expect",
]
```

```
./.gatekeeper/run fixture core/hygiene/shared-names --to crates/cli/tests/golden/private-names
```

No file joins `frozen`: the hot file this work touches, `ci.yml`, is frozen by #1587's own lines.

Gatekeeper's lines are in since G4 and G5: the `[[collect]]` for the folder of arch's parts, and the two bans on
`tool = "drover"` and `{tool:drover}` under its packs folder. The cap's removal needs no line in either repo.
