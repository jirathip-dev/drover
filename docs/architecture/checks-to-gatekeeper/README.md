# The 4 checks move into gatekeeper's core pack (#1643)

## Summary

Decision: gatekeeper owns four checks as its own code: rules, arch, hygiene and PR body. The line cap is no longer one of them: the owner removed it (#1812). Gatekeeper deletes the cap check it already built (gatekeeper#127), and each repo drops its cap files in the same PR that bumps its gatekeeper pin. Everything else stands as approved.
Slices: done: gatekeeper's six, the session-id fix (#1615), the done gate through gatekeeper. Left: one PR per repo (pin 1.5.0 or later, cap files out), drover's own adoption (#1587), the lesson lane reading gatekeeper's report, last the four verbs deleted.
Open questions: (1) #1587 is held until #1643 merges, but #1643's own PR is the last slice and waits for #1587: swap that block? (2) After a repo's bump, a lane's own check in its pane fails the names gate, as the pane has no names list (#1642 was closed): leave it, since the done gate decides?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read afterwards: a lane, `turn done`, the lesson lane, a gate file, a repo's one PR |
| `adr.md` | the question, the options, the 13 decisions, consequences, the slices for both repos with their state, #1587, #1615 and #1642, the open questions, the `arch.toml` lines |
| `shapes.md` | the typed inputs of a check, the rules report, the gate files, who owns which repo file, drover's one gatekeeper module, the shared names set |
| `boundaries.md` | which module may call which in both repos, and four statecharts |

Approved 2026-11-23 (q3305), read on drover staging 7c0a28c1c and gatekeeper 1.2.0. Revised 2026-11-26 on the
owner's amendment (q3842), read on drover staging 53d0dd927 and gatekeeper 1.5.0: the cap check is removed
(#1812), so 5 checks become 4.

## What the revision of 2026-11-26 changes

| Was | Is |
|---|---|
| Five checks move, `core/cap` among them | Four. The cap is removed in both repos by its own issues: #1812 in drover, gatekeeper#127 in gatekeeper |
| The merge check runs `core/cap` through the wrapper (decision 8) | Withdrawn. #1812 removes the merge check's cap command, which D3 built |
| Every repo: one pin bump to 1.4.0 | Every repo: one PR that pins the newest release (1.5.0 or later) and deletes its cap files. 1.4.0 was never released |
| Drover keeps the cap view | Nothing of the cap stays |
| D1 writes the shared names set in drover | D1 merged without it. D2 generates drover's copy from the pin and adds the guard's test |
| D5 deletes five verbs | Four. `cap-check` is #1812's |
| Open question: gatekeeper's cap raise | Closed. Two new ones, above |

## The design on one screen

```
 before                                           after
 ──────                                           ─────
 gatekeeper core pack                             gatekeeper core pack
   core/rules    ─▶ pinned drover binary            core/rules    check = "rules"    ─▶ checks.json
   core/arch     ─▶ pinned drover binary            core/arch     check = "arch"
   core/hygiene  ─▶ pinned drover ─▶ fleet.toml     core/hygiene  check = "hygiene"  ◀─ names: one env var
   core/pr-body  ─▶ pinned drover binary            core/pr-body  check = "pr-body"  ◀─ --pr-body, the head
   core/cap      ─▶ pinned drover binary            (gone: the line cap is removed, #1812)

 drover turn done                                 drover turn done
   its own copy of the PR body rule                 writes the PR body to the lane's host
   ./.gatekeeper/run check, or its arch verb        names on stdin ─▶ ./.gatekeeper/run check --pr-body …
                                                    nothing else: no check code left in drover
```

| Before | After |
|---|---|
| Every CI job and lane fetches a 17.6 MB drover build to run the checks | The checks are in the gatekeeper binary the wrapper fetches anyway |
| A check fix is a drover release, a gatekeeper release, then a bump per repo | A gatekeeper release, then a bump per repo |
| The names check reads the fleet's config, so it fails on a remote host (#1642) | It reads one env var; drover's done gate sets it on any host |
| The PR body rule is written three times (done gate, a verb, a CI script) | Once, in gatekeeper |
| Drover's own adoption builds drover first to check itself (#1587's PR) | It pins a release like any repo |
| Shadow lesson checks are run by drover at a merge, active ones by CI | Gatekeeper runs both and writes one report; drover reads it to promote or retire |
| The guard and the gate find a private name with one function | Each repo has its own, and both must pass one shared set of cases |

## What stays drover's

Lesson promotion and retirement (the lesson lane), the pre-send leak guard (`PUBLIC_LEAK`: the hook, pushes, wip
saves, drover's own issue and PR text), `hygiene scan` and `hygiene push`, hotspots. All of these need the fleet's
config, the event log or the queue, which gatekeeper never reads. The guard keeps its own private-name matcher;
the shared names set holds it to gatekeeper's.

## What the owner decides

Approve, revise or reject this revision, and answer its two open questions (`adr.md`, "Open question").
Decision 13 is the new one: a repo's cap files leave in the same PR that bumps its gatekeeper pin, because every
release so far fails `core/cap` once `ci/cap.toml` is gone. Two issues need an amendment from it: #1587 (the
release is 1.5.0, the shared names set, no cap) and each repo's cap-removal issue (it carries the pin bump).
