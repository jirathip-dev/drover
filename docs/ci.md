# Affected-only CI (#947)

New fleets run affected-only CI: a pull request runs only the checks its change can affect. The full run happens when
drover adds the fleet's `merge_label` on a PASS at the head. Copy the recipe from [templates/ci/](../templates/ci/).

| File | Copy to | What it does |
| --- | --- | --- |
| `templates/ci/ci.yml` | your workflows folder | affected-only PR runs, full on the label, staging reuse, one `ci` check |
| `templates/ci/nightly-red.yml` | your workflows folder | nightly full run of the integration branch, one `nightly red` issue |

In both files, replace `staging` with your integration branch. Then fill in the `leg` job's commands: each one fails
with "replace with your … command" until you do.

## The model

- **Affected-only PR runs.** A ready PR into the integration branch runs the `changes` job, then only the legs it
  picked. A draft runs nothing.
- **Full run on the label.** When the PR carries the label (`merge-ready`, or the repo variable `MERGE_LABEL`), the
  same `pull_request` run goes full: every leg. It has to be a `pull_request` run, because only `pull_request` suites
  count for required checks. drover adds the label on a PASS at the head and removes it when the head moves.
- **One required check.** The `ci` job is red unless every leg passed or the scope step left it out. Require only
  `ci`.

## The scope contract

`changes` is a small step you can replace. Its outputs are the contract:

- `legs`: a JSON list, a subset of `LEGS` (`["lint","test","build"]` in the template). `[]` means nothing to run.
- `tree`: the tree the run tested (`git rev-parse HEAD^{tree}`).

The template's placeholder rule:

- Docs only (`*.md`, `docs/`): no legs.
- Root config, a lockfile, `.github/`, or toolchain files (`rust-toolchain*`, `.tool-versions`, `mise.toml`,
  `.nvmrc`): the full run.
- Anything else: every leg.

Swap in your own build graph (Nx affected, a cargo package graph, …). Keep the two outputs and the force-full rule.

## Concurrency and the no-op rule

Each PR has one concurrency group, so a new push or the label cancels that PR's older run. A `labeled` event for
any other label has to be a no-op that never cancels a full run. These are the groups the `concurrency.group`
expression gives:

| Event | Group | Cancels |
| --- | --- | --- |
| `pull_request` opened / synchronize / reopened / ready_for_review | `ci-pr-<n>` | the PR's older run |
| `pull_request` labeled with the merge label | `ci-pr-<n>` | the PR's narrowed run, which goes full |
| `pull_request` labeled with any other label, or a draft | `ci-run-<run_id>` | nothing (a no-op) |
| `push` to the integration branch | `ci-refs/heads/<branch>` | nothing (queued) |
| `workflow_dispatch`, nightly (`schedule` via `workflow_call`) | `ci-run-<run_id>` | nothing |

A no-op run skips every leg, and its aggregate job is named `no-op`, not `ci`. So it never posts a `ci` result over a
full run's result on the same head. `crates/cli/tests/ci_templates.rs` parses both templates and checks the group's
label rule. To check the YAML by hand:

```sh
ruby -ryaml -e 'ARGV.each { |f| YAML.safe_load(File.read(f)); puts "#{f}: ok" }' templates/ci/*.yml
actionlint templates/ci/*.yml   # if you have it
```

## Staging reuse

A green full PR run uploads a `ci-full-<tree>` artifact. When a push to the integration branch has the same tree as
a green full labelled PR run, the `reuse` job finds that artifact. The push run then skips the legs, and `ci` names
the run it reused. This happens when the merged PR was already up to date with the branch.

The decision fails closed. Any error (an API failure, no artifact, an expired one, a run that isn't green) leaves
`reuse` empty, and the push gets the full run.

## Nightly red

`nightly-red.yml` calls `ci.yml` every night on the integration branch, as a full run.

- **Red:** it opens one issue titled and labelled `nightly red`, or comments on the one that is already open.
- **Green:** it closes that issue.

## Wiring it to drover

```toml
[fleet.app]
required_checks = ["ci"]          # drover's gates read only the aggregate
merge_label = "merge-ready"       # drover adds it on a PASS at the head: the full run
```

`drover setup` creates the label in the repo if it is missing. `drover doctor` shows ✗ with the
`gh label create … --force` command when it is missing. After drover labels a PR, it waits one drive pass for the
full run to start before it merges.

### With `[fleet.<n>.land] mode = "fast-first"`

Fast-first lands a PASS without the label's full run ([config.md](config.md#fast-first-landing-350)). Set
`fast = ["ci"]`: the narrowed PR run is the fast check. The push to the integration branch then has no green full PR
run to reuse, so it runs in full. That run is the "full suite on integration's tip" fast-first reads to resume, or to
find and revert the culprit. Keep `.github/**` and lockfiles in `full_first_paths`. Those PRs land on the full suite.

## Forcing a full run

- **On a PR:** add the merge label by hand (`gh pr edit <n> --add-label merge-ready`). Or touch a force-full path.
- **On a branch:** run the workflow by hand (`gh workflow run ci.yml --ref <branch>`). A `workflow_dispatch` run
  is always full.
- **Nightly now:** `gh workflow run nightly-red.yml`.
