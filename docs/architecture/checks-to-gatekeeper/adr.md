# ADR: the 5 checks move into gatekeeper's core pack; drover runs them through gatekeeper (#1643)

## Summary

Decision: gatekeeper gets the five checks as its own code, with the same findings. The files they read keep their paths. Drover keeps the cap view, lesson promotion and the pre-send leak guard. Its done gate runs gatekeeper on the lane's host and hands it the PR body and the names list. Private names are matched in two places, the guard and gatekeeper's check: one shared set of cases, which both must pass, keeps them the same.
Slices: gatekeeper: inputs, the five checks, release 1.4.0. Every repo: one pin bump. Drover: #1615 with the shared names set, #1587 at 1.4.0, the done gate and lessons through gatekeeper, last the five verbs deleted.
Open questions: gatekeeper's line cap has 4 lines left on staging and about 1,500 arrive. Approve one raise, 7,700 to 9,500?

Status: proposed 2026-11-23 (architect lane), revised the same day on the owner's note (q3300): the release is
1.4.0, and the two name matchers share one set of cases. Read `README.md` first.

## Question

Gatekeeper's core pack runs five of its gates by calling a pinned drover binary. Where does the code of those
checks live, what may it read, how does drover's done gate run it on a host that has none of drover's config, and
in which order do two repos and their consumers change so that no repo is checked less on any day?

## Where it stands (drover staging 7c0a28c1c, gatekeeper main aa132f9 and staging 5b69691)

| Gate | Drover verb it calls | The code in drover | Lines that move |
|---|---|---|---|
| `core/cap` | `cap-check` | part of `overlap.rs` | about 150 |
| `core/rules` | `lesson-check` | part of `lesson.rs` | about 75 |
| `core/arch` | `arch-check` | `arch.rs` | about 690 of 703 |
| `core/hygiene` | `hygiene check` | part of `hygiene.rs` | about 95 of 527 |
| `core/pr-body` | `pr-body` | part of `done.rs` | about 75 |

- The pack pins one drover build in its tool file. The binary is 17.6 MB per target. A fix to a check is a drover
  release, a gatekeeper release and a bump per repo.
- Gatekeeper's boundaries doc has the rule "A pack calls a drover concept through drover. gatekeeper never
  re-implements one." The owner's decision of 2026-11-23 reverses it.
- `hygiene check` reads the fleet's `private_names` from the fleet config on the host it runs on. A remote host has
  none, so the gate fails `list-missing` there (#1642).
- The PR body rule is written three times: `turn done`'s own check (refusal `PR_BODY`), the `pr-body` verb, and
  the shell script behind the `evidence` workflow.
- Drover's adoption PR (#1634 for #1587) builds drover before it can check itself: a `[tools]` line, a wrapper
  script, a 600 s budget for `core/arch`, and a local gate for the `Cap:` trailer, which `core/cap` does not check
  because the gate passes no base.
- `arch-check`'s `[docs] cli = true` checks docs against drover's own command tree. Only drover's repo sets it.
- Only part of `hygiene.rs` is the gate. The rest is the pre-send guard (`PUBLIC_LEAK`), `hygiene scan` and
  `hygiene push`. #1615's `session-id` pattern is in the guard, not in the gate.
- The guard and the gate find a private name with one function today: a whole word, any case, reported by its
  number in the list. The guard needs it before every send, with no repo wrapper at hand, so after the move the
  rule is written in both repos.
- Drover's merge runs `cap-check` on the merged tree and runs the shadow lesson checks itself to record their
  history; the lesson lane promotes or retires by that history.
- A built-in check in gatekeeper is one file in a collected folder with a generated module list. Every gate has
  fixture cases with the text each must print. Gatekeeper's files are capped at 400 lines, its functions at 60.
- Gatekeeper's line cap is 7,700. Staging counts 7,696 (`drover cap gatekeeper`), main 7,641.
- Gatekeeper's last tag is 1.2.0. Its staging is at 1.3.0 already (#71), so this work's release is 1.4.0.
- Repos on gatekeeper today (integration branches): blocks and swag at 1.0.0, which has no `core/hygiene` or
  `core/pr-body` yet; cooee at 1.1.0, shed at 1.1.1, loom, sendmeter and one private repo at 1.1.2. Not yet: drover (#1587), beacon, farrier, scene-esp32, compact. No repo
  still has the pre-rename config folder.

## Options

### 1. Where the code lives

| Option | Verdict |
|---|---|
| A. Stay: a pinned drover, and ship the config it needs to every host (#1642 alone) | No. Every job still fetches drover, every fix still needs two releases, and drover's own repo still builds itself first. The owner decided against it. |
| B. Split a small checks binary out of drover and pin that | No. A smaller download, the same two-release path, and the checks are still drover's. |
| C. Gatekeeper links a crate kept in drover's repo | No. One copy, but gatekeeper's build then hangs on drover's repo and its libraries. Gatekeeper links no other repo's code today. |
| D. Port: five built-in checks in gatekeeper, printing what they print today | **Yes.** |

### 2. The files the checks read

| Option | Verdict |
|---|---|
| A. Keep the paths: `ci/cap.toml`, `ci/cap.d/`, `.drover/rules.md`, `.drover/arch.toml`, the arch baseline | **Yes.** Drover still reads all three (cap view, briefs, hotspots). A check ported with its files in place is proven by fixtures nobody edits. |
| B. Move them under gatekeeper's folder | No. Every repo, every role prompt, the lane hook's cap guard and hotspots change for no change in behaviour. It can be its own issue later. |

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
8. **The merge check** runs the wrapper for `core/cap` on the merged tree in place of drover's verb.
9. **Lessons** (option 5A). The `rules` check runs shadow and active checks and writes `checks.json`. Drover's
   merge reads it for the `lesson-shadow` event; `lesson apply` reads it for the lines to rewrite. Drover knows the
   two marks it writes, `shadow since` and `active since`, and no more of the line format.
10. **The arch baseline** is rewritten by gatekeeper's `baseline` verb and follows gatekeeper's rule: after the
    first write it only shrinks.
11. **One module in drover names gatekeeper.** It builds the wrapper's command line and reads its report. An arch
    ban keeps the wrapper's path out of every other file.
12. **One shared names set** (option 6C). A names list, lines of text and the hit each line must give
    (`shapes.md`). Gatekeeper holds it as a fixture case of `core/hygiene`, so its selftest runs it. Drover holds
    the same bytes and one test runs the guard over them. Drover writes the set first, from today's matcher (D1);
    gatekeeper copies it in with the port (G3) and is its home from then on. Once drover pins 1.4.0 (D2) its copy
    is a generated file, written from the pinned release, so a copy that differs fails `core/arch`.

| | Moves to gatekeeper | Stays in drover |
|---|---|---|
| cap | counting the lines, the cap and its raises, the `Cap:` trailer on a commit that edits the cap | `drover cap` (the queue view), `cap log`, the `cap-near` alert, the hook's refusal to edit the cap from a lane |
| rules | the line format, the 30-line limit, running each lesson check | the lesson lane: digest, the history, promote and retire, the `lesson-shadow` event |
| arch | all of it: layers, bans, sizes, frozen, generated, shared, comments, stale docs, the baseline | hotspots reading three list keys; the docs-against-CLI test |
| hygiene | the gate: tree, commit messages and PR body against the names; the shared names set's home | the pre-send guard (`PUBLIC_LEAK`) with its own name matcher, `hygiene scan`, `hygiene push`, doctor's line |
| PR body | all of it: names the head, has `## For humans`, no "Generated with" line | nothing |

## Consequences

- A lane's host and a CI job fetch one binary. The drover tool file leaves the core pack, so a config that still
  names it is refused.
- A check fix ships with a gatekeeper release. Drover releases no longer gate the gates.
- Three things a lane sees change. (a) The `PR_BODY` refusal code is gone; the same fault is `DONE_CHECK` with
  gatekeeper's lines. (b) `turn done` in a repo with neither the wrapper nor `done_checks` is refused, where today
  it passes with whatever fallback applies. (c) The arch baseline cannot grow: a redo on the owner's ask deletes the
  file first.
- The merge check also runs the active lesson checks on the merged tree, since one run produces the report.
- Drover still parses two things gatekeeper owns: the cap number in `ci/cap.toml` for its view, and three list
  keys of `.drover/arch.toml` for hotspots. Both are typed reads of a few keys.
- Arch's patterns are read as gatekeeper globs. Where a repo's pattern means something else there, the parity run
  shows it and that repo's bump PR rewrites the pattern.
- The rule for a whole-word name match is written twice. A change to it is a gatekeeper PR that edits the set and
  its matcher; drover's next bump PR regenerates its copy and its guard must pass it in that PR. An edit of drover's
  copy alone fails `core/arch`.
- Gatekeeper gains one verb, `fixture`, which writes a fixture case's files to a folder: drover's generator.
- Drover loses about 1,150 lines and gains about 200 (the module, the session ids): about 950 net, and about 2,000
  lines of tests leave with the checks. Gatekeeper gains about 1,500.
- Until a repo bumps, its pinned drover build keeps working: the done gate's env var and `--pr-body` are read by
  old and new releases alike (1.0.0 accepts both and has neither gate).

## Slices

Gatekeeper (six issues to file there). G2, G3 and G4 can run side by side: each adds files to collected folders.

| # | Issue | Blocked on | AC |
|---|---|---|---|
| G0 | cap: raise gatekeeper to 9,500 (filed by the orchestrator, as every raise is) | the owner's answer below | the raise file with its reason; nothing else |
| G1 | core checks, the frame: the names, the head, the PR body and the integration branch reach a built-in check as typed values; the boundaries rule reversed; this ADR copied in | G0 | selftest feeds a case's `env` file into the typed values; the names type cannot be printed; docs say the checks are gatekeeper's |
| G2 | `core/pr-body`, `core/cap`, `core/rules` as built-in checks | G1 | each gate file has `check`, no tool; no `expect` file edited; drover's cases added; `core/cap` fails a cap edit without its trailer; `rules` writes `checks.json` |
| G3 | `core/hygiene` as a built-in check, with the shared names set | G1, D1 | names only from the typed value; `list-missing` as today; `[off]` for it refused in a public repo's CI run; the shared names set is case `core/hygiene/shared-names`, the same bytes as drover's from D1, and selftest passes it; `fixture <pack>/<gate>/<case> --to <dir>` writes a case's files |
| G4 | `core/arch` as a built-in check, with `baseline core/arch` | G1 | no `expect` file edited; drover's 24 arch tests added as cases; old and new print the same findings on every consumer's integration head (table in the PR body) |
| G5 | the drover tool leaves the core pack; release 1.4.0 | G2, G3, G4 | `git grep` finds no drover verb in `packs/`; the two bans in "arch.toml lines" are in; the skill is regenerated |

Every repo on gatekeeper (one issue each, after G5): `./.gatekeeper/run pin 1.4.0`. A private repo adds the `[off]`
line for `core/hygiene`. The four repos not on gatekeeper adopt at 1.4.0, in gatekeeper's adoption order. 1.3.0
(#71, on gatekeeper's staging) still calls drover for the five checks: a repo may pin it meanwhile, and bumps again.

Drover:

| # | Issue | Blocked on | AC |
|---|---|---|---|
| D1 | #1615: `session-id` hits only a known session id; the shared names set | nothing | #1615's own AC, and: the shared names set is in drover, written from today's matcher, and one test runs the guard over it and gets its `expect` file |
| D2 | #1587, amended: adopt at 1.4.0 | G5 | #1587's AC, and: no `[tools]` line, no script that builds drover, no local `Cap:` trailer gate; `[docs] cli` is a test; the `evidence` workflow and its script are deleted; the shared names set is a generated file, written from the pin |
| D3 | the done gate and the merge check run the wrapper, through one module (to file) | nothing | a test: a lane on a remote host with no fleet config passes `turn done`, the names on stdin and in no argument, file or log; the merge check runs `core/cap` through the wrapper; drover's own PR body check still runs |
| D4 | the lesson lane reads gatekeeper's report (to file) | every bump | the merge's `lesson-shadow` event and `lesson apply` come from `checks.json`; drover runs no lesson check itself |
| D5 | #1643's own PR: the five verbs and their code are deleted | D1 to D4, every bump, the four adoptions | `git grep` finds none of the five verbs in drover's code, workflows or roles; drover's own PR body check is gone and a bad body is `DONE_CHECK` naming `core/pr-body`; the remote-host test of D3 still passes; setup's baseline step calls the wrapper; the PR body names #1587 and #1615 with their commits |

D3 keeps drover's own PR body check, and the old path for a repo without the wrapper. D5 removes both, once every
repo's pin has `core/pr-body`: so no repo is checked less on any day. D1 and D3 can start on approval. D1 is the one
drover slice a gatekeeper slice waits for: G3 copies the set D1 writes.

## The folded issues

- **#1587** is fixed by D2. It needs an amendment before its lane continues: pin 1.4.0 in place of 1.1.0, and the
  extra AC lines in D2's row. Its spec keeps the `evidence` workflow; with `core/pr-body` in the gates that
  workflow is a second copy, so D2 deletes it and its PR body names the check to drop from branch protection.
- **#1615** is not made moot by the move: its pattern is in the pre-send guard, which stays. It is fixed by D1, as
  its own spec says. The guard's check gains the known session ids (`shapes.md`). Its PR also writes the shared
  names set, since it is the slice that edits the guard's patterns.
- **#1642** (under #1646) is half solved here: D3's done gate passes on a remote host. The lane's own run in its
  pane still needs the names in its env file, which is #1642's spawn change.

## Open question

1. Gatekeeper's staging counts 7,696 lines against a cap of 7,700 (main counted 7,641 before #71). About 1,100
   lines move; at gatekeeper's line width and function limit, with the typed inputs, the report, the baseline
   writer and the `fixture` verb, that is about 1,500, so about 9,200. Approve one raise to 9,500, as its own issue
   (G0) ahead of the others? Drover's count falls by about 950. (The first draft asked for 9,400, on main's count.)

## `.drover/arch.toml` lines

Drover, in D3 (the module) and D2 (the key and the three generated files):

```toml
[[ban]]
pattern = ".gatekeeper/run"
paths = ["crates/*/src/**"]
except = ["crates/core/src/gatekeeper.rs"]
reason = "#1643: one module builds the gatekeeper command and reads its report"

[docs]
paths = ["docs/**/*.md", "README.md"]
watch = ["crates/core/src/cli.rs", "crates/core/src/config.rs", "crates/server/src/api.rs"]
skip = [".drover/**", "roles/usage-tail.md", "scripts/test-shards.txt"]
```

The second block is today's `[docs]` without `cli = true`. D2 also adds three paths to the `generated` list, and
one line to `just gen`, the `generate` command:

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

Gatekeeper, in G4 (the folder) and G5 (the bans):

```toml
[[collect]]
dir = "crates/gatekeeper-engine/src/arch"
glob = "*.rs"

[[ban]]
pattern = "tool = \"drover\""
paths = ["packs/**"]
reason = "the core pack's checks are built in; no gate calls drover"

[[ban]]
pattern = "{tool:drover}"
paths = ["packs/**"]
reason = "the core pack's checks are built in; no gate calls drover"
```

The five new check files, the `fixture` verb's file and the shared set's case need no line: the check, verb and
fixture folders are collected already, and the two module lists are generated by gatekeeper's `sh scripts/gen.sh`.
