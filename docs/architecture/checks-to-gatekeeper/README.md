# The 5 checks move into gatekeeper's core pack (#1643)

## Summary

Decision: gatekeeper gets the five checks as its own code, with the same findings. The files they read keep their paths. Drover keeps the cap view, lesson promotion and the pre-send leak guard. Its done gate runs gatekeeper on the lane's host and hands it the PR body and the names list. Private names are matched in two places, the guard and gatekeeper's check: one shared set of cases, which both must pass, keeps them the same.
Slices: gatekeeper: inputs, the five checks, release 1.4.0. Every repo: one pin bump. Drover: #1615 with the shared names set, #1587 at 1.4.0, the done gate and lessons through gatekeeper, last the five verbs deleted.
Open questions: gatekeeper's line cap has 4 lines left on staging and about 1,500 arrive. Approve one raise, 7,700 to 9,500?

## Read in this order

| File | What |
|---|---|
| `usage.md` | how the callers read afterwards: a lane, `turn done`, the merge check, the lesson lane, a gate file, a repo's bump |
| `adr.md` | the question, the options, the 12 decisions, consequences, the slices for both repos, #1587 and #1615, the open question, the `arch.toml` lines |
| `shapes.md` | the typed inputs of a check, the rules report, the gate files, who owns which repo file, drover's one gatekeeper module, the shared names set |
| `boundaries.md` | which module may call which in both repos, and three statecharts |

Read on drover staging 7c0a28c1c and gatekeeper main aa132f9 (v1.2.0), 2026-11-23. Revised the same day on the
owner's note (q3300), against gatekeeper staging 5b69691 (1.3.0, #71): the release is 1.4.0, and the guard and
`core/hygiene` share one set of name cases.

## The design on one screen

```
 today                                            after
 ─────                                            ─────
 gatekeeper core pack                             gatekeeper core pack
   core/cap      ─▶ pinned drover binary            core/cap      check = "cap"
   core/rules    ─▶ pinned drover binary            core/rules    check = "rules"    ─▶ checks.json
   core/arch     ─▶ pinned drover binary            core/arch     check = "arch"
   core/hygiene  ─▶ pinned drover ─▶ fleet.toml     core/hygiene  check = "hygiene"  ◀─ names: one env var
   core/pr-body  ─▶ pinned drover binary            core/pr-body  check = "pr-body"  ◀─ --pr-body, the head

 drover turn done                                 drover turn done
   its own copy of the PR body rule                 writes the PR body to the lane's host
   ./.gatekeeper/run check, or its arch verb        names on stdin ─▶ ./.gatekeeper/run check --pr-body …
                                                    nothing else: no check code left in drover
```

| Today | After |
|---|---|
| Every CI job and lane fetches a 17.6 MB drover build to run five checks | The checks are in the gatekeeper binary the wrapper fetches anyway |
| A check fix is a drover release, a gatekeeper release, then a bump per repo | A gatekeeper release, then a bump per repo |
| The names check reads the fleet's config, so it fails on a remote host (#1642) | It reads one env var; drover's done gate sets it on any host |
| The PR body rule is written three times (done gate, a verb, a CI script) | Once, in gatekeeper |
| Drover's own adoption builds drover first to check itself (#1587's PR) | It pins a release like any repo |
| Shadow lesson checks are run by drover at a merge, active ones by CI | Gatekeeper runs both and writes one report; drover reads it to promote or retire |
| The guard and the gate find a private name with one function | Each repo has its own, and both must pass one shared set of cases |

## What stays drover's

The cap view over the queue (`drover cap`, the `cap-near` alert), lesson promotion and retirement (the lesson lane),
the pre-send leak guard (`PUBLIC_LEAK`: the hook, pushes, wip saves, drover's own issue and PR text), `hygiene scan`
and `hygiene push`, hotspots. All of these need the fleet's config, the event log or the queue, which gatekeeper
never reads. The guard keeps its own private-name matcher; the shared names set holds it to gatekeeper's.

## What the owner decides

Approve, revise or reject the ADR, and answer its one open question: the size of gatekeeper's one cap raise.
Three choices in it change what a lane sees and are listed under "Consequences" in `adr.md`: the `PR_BODY` refusal
becomes a `DONE_CHECK` naming `core/pr-body`; a repo with neither gatekeeper nor `done_checks` is refused at
`turn done`; the arch baseline can only shrink.
