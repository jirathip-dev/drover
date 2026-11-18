# ADR: one install path, from CI's signed release builds (#1275)

## Summary

Decision: CI builds, signs and publishes every green main and staging build as a GitHub release. The 5-minute self-update job becomes the only installer: in one pass it downloads and checks the build on every host, then swaps the boats and the Mini last. A host on another build is passed over quietly until it is caught up. Nothing builds drover from source.
Slices: 1 the release workflow (build, sign, notarize, publish, prune), 2 the one install path (download, checksum, version, doctor gate, swap), 3 the staged rollout with the 30-minute lag rule and the quiet skip, 4 the source build and local signing removed, the docs on one page.
Open questions: (1) Keep the signing secrets in a GitHub environment limited to main and staging, not as plain repo secrets? Any PR branch's workflow can read a plain repo secret, and lanes push PR branches. I recommend the environment. (2) `self-update-waiting` alerts after 45 minutes without a build, because every promotion waits about 20 minutes for CI anyway; say if you want it at once. (3) The AC's plain `spctl -a -vv` rejects every bare command-line binary, notarized or not (measured); the check becomes `spctl -a -vv -t install`.

## Question

The owner decided the what (t135, q1945 to q1951): prebuilt signed builds, staged on every host, swapped together, a
missing build waits, a lagging host is left after 30 minutes, the staging serve follows prereleases. This ADR decides
the how: who installs, where the rollout's state lives, how a host gets its build, how the seconds of the swap stay
free of `ACP_UNGUARDED`, and how the release workflow runs without touching landing.

## Why it breaks today

Two installers on two clocks. `update::install` swaps the Mini as soon as its own build is done. `hosts::sync`, run
by `check`'s survey, then finds each boat on another version and starts a `cargo install` there. `hosts::acp_guard`
rightly refuses a lane while versions differ, and its refusal marks the host sick, which placement reads as
`NO_HOST` until the next health look. The skew is built in: it lasts as long as the slowest boat's build.

## Decisions

### 1. One installer: the self-update job

The rollout over every host lives in `update`. `hosts::sync`, its background job on the host and its
`host-sync-skipped` event are deleted, not converted. The survey keeps one cheap read per host, its `--version`, for
`drover hosts` and to look at a host's health again when its build changed.

Considered: keep two installers and make them agree through the log. Rejected: two passes on two timers cannot swap
"together", and every fix would be a new mark both must honour.

### 2. State is what is on the hosts, plus a few marks in the log

Each pass reads two versions per host in one call: the live binary's and `drover.new`'s. The log holds only what
those cannot say: when the head was first seen (`self-update-seen`, the wait clock), when its staging began
(`self-update-staging`, the lag clock), a swap in flight (`self-update-swapping`), who was left (`host-behind`),
and what was already alerted. There is no rollout file and
no `sha` marker file. The next step is a pure function of these (`step`, `shapes.md`), so a pass that dies resumes,
and running a pass twice does what running it once did.

### 3. Each host downloads its own asset; the Mini hands it the checksum

The Mini reads the release's `SHA256SUMS` once per pass with `gh`. Each host then gets one shell line with its
asset's public URL and expected hash: `curl` to `drover.new.part`, check the hash, `chmod`, rename to `drover.new`,
print its `--version`. The Mini compares the version to the head's sha and, where a fleet.toml is, runs
`drover.new doctor --config-only`. Any failure removes `drover.new`; the live binary is never written before a swap.

Considered: the Mini downloads every asset and copies it over ssh. Rejected: a second transport and 13 MB per boat
through the Mini on every rollout, for hosts that already reach GitHub. Considered: each host fetches `SHA256SUMS`
itself. Rejected: five reads of one fact.

The doctor gate is `--config-only` alone. `doctor` has had no `--tokens` since #707 retired the secret store; the
word survives only in the dry run's text, which goes. A boat has no fleet.toml (#592), so its gate is the checksum
and the version: the version call proves the binary runs on that OS, CPU and libc.

### 4. The swap window is closed by one mark and one typed refusal

Swapping the boats before the Mini leaves seconds where a boat is ahead of the process that places lanes, and an
old drive pass can outlive the Mini's swap. Both are covered the same way:

- The pass logs `self-update-swapping` before the first boat swaps. Until `self-updated` follows (or the mark is
  older than its limit), placement passes over every remote host.
- `acp_guard` returns a typed reason. A build difference is `Unguarded::Build`, which spawn refuses as
  `HOST_UPDATING`: the lane still does not start, but nothing is wrong with the host, so it is not marked sick and
  it is not an owner refusal. `ACP_UNGUARDED` stays for a missing hook or adapter. The health row for a build
  difference becomes the `!` warning non-ACP hosts already get, never `✗`.
- `HOST_UPDATING` is a host hold like `HOST_BUSY`: placement ranks the host last, so another host that fits takes
  the lane; with none, the issue waits and says why.

Any build skew, whatever caused it (a host left behind, a host just switched on, a hand install), is healed by the
same job: a pass that finds an enabled, reachable host off the Mini's build stages and swaps it.

Considered: installs addressed by version, the Mini calling each boat's binary of its own sha. Rejected: lanes and
hooks call `drover` by PATH, so a per-host flip remains, and every launchd job, hook and link that names the binary
would change. Considered: the swap takes drive's lock. Rejected: an orchestrator's own `drover spawn` and a pass
already running hold no such lock.

### 5. The lag rule

- Waited for: an enabled host that should be up. Not waited for: a disabled host, and a cloud host that is not
  running. Those are left at once and caught up when they start (`cloud::start` runs the same install before the
  host reports up).
- The clock starts at `self-update-staging`, logged once when the head's release is first found. After
  `self_update_lag_minutes` (30) the staged hosts swap; each other one gets one `host-behind`.
- The Mini is never left: if its own stage fails, nobody swaps (`Stuck`), `self-update-failed` alerts once per host
  and head, and each pass tries again. A shed protocol mismatch keeps its once-per-mismatch key.
- A new head replaces the rollout: staging starts over for it. Only the head is ever installed.

### 6. The release workflow runs after CI and never fails a commit

- **Trigger:** `workflow_run` of `ci`, completed with success, for a push to main or staging. "CI green" is then
  exact with no list of job names to keep in step, and `.github/workflows/ci.yml` is not edited. staging is the
  default branch, so the workflow is live from its merge and is proven on staging pushes before any promotion.
- **It must not turn staging red.** `land::suite` counts every check run on integration's tip (the drover fleet
  sets no `required_checks`), and a red tip pauses landing and hunts a culprit (`crates/core/src/land.rs`). A run
  started by `workflow_run` hangs its checks on the default branch's head. So a failed build, signing or publish
  step ends its job green with an error annotation and publishes nothing. The failure signal is the one drover
  already consumes: no release, `self-update-waiting`. The owner re-runs the workflow run; the next pass installs.
- **No `concurrency` key:** a cancelled run's check reads as failed.
- **Secrets:** only the macOS job names them, through the `release` environment (open question 1). The run is
  refused unless the triggering run's event is `push`, so a fork's pull request can never reach them.
- **Publish:** one `gh release create` with both binaries and `SHA256SUMS`. `gh` uploads to a draft and publishes
  last, so a release that exists is complete. Tags: `build-<sha7>` for main, kept for good;
  `build-staging-<sha7>` as a prerelease. The staging run then deletes prereleases older than 14 days, tag included.
- **Linux is built on the oldest hosted Ubuntu image** to keep the glibc floor low.

Considered: `on: push` with a publish job that polls for CI. Rejected: a polling job, and its checks sit pending
on the pushed staging commit. Considered: a last job in `ci.yml` that needs every other job. Rejected: a `needs`
list that goes stale, and a notarization hiccup would be red CI on staging.

### 7. Signing

CI signs the macOS binary with the Developer ID Application certificate, hardened runtime, a secure timestamp and
the identifier `drover`, then notarizes a zip of it with `notarytool` and the App Store Connect API key. A bare
Mach-O cannot carry a stapled ticket (`facts.md`), so the notarization is recorded with Apple and checked online.
The job verifies before it uploads, and only the signed binary is uploaded.

On a host nothing is signed any more. `update`'s "drover-local" step and `scripts/install-gate.sh` go; doctor's
signature row checks for a Developer ID Application signature with the identifier `drover`.
`scripts/signing-identity.sh` stays: shed and cooee installs still sign with that identity.

### 8. The waiting alert has a grace

Every promotion has no release for about 20 minutes: CI, then the build. An alert each time would teach the owner
to ignore it. The pass prints that it waits from the first minute, and logs `self-update-waiting` once per head
when the release is still missing `self_update_wait_minutes` (45) after the head was first seen (open question 2).

## Consequences

- A promotion costs the boats nothing: every host runs the old build until all are staged, then the swap takes
  seconds, inside which remote placement holds.
- No release, no update. A broken signing secret or an expired certificate stops updates everywhere until fixed;
  the fleet keeps working on the old build and one alert says so. There is no source-build fallback, by q1949.
- The first promotion that carries this is the last source build, and the boats wait for its release (README,
  owner step 3).
- A host needs `curl` and `sha256sum` or `shasum`, and no Rust toolchain for drover (shed still installs by cargo).
- A CPU and OS pair with no asset (an Intel Mac, an ARM Linux box) fails its stage with that reason and stays
  behind. Adding a target is one matrix line.
- Every green staging push builds on a macOS runner (free on a public repo) and adds a `build-staging-*` tag that
  clones fetch; they are pruned after 14 days. Main's tags stay.
- A release run in flight shows as a pending check on staging's head for a few minutes, which can delay a
  `land-green` mark; it can never be red.
- New: one refusal code (`HOST_UPDATING`, in the usage doc, so `just gen` reruns the brief goldens), four
  `[limits]` keys, events `self-update-seen`, `self-update-waiting`, `self-update-staging`,
  `self-update-swapping`, `host-behind`.
  Gone: `host-sync-skipped`, the `sha` marker file, `install_line`, the build folders under the state directory.

## Hot files

This issue converts no central list. It keeps two shared files from growing: the release pipeline is its own
workflow file, and releases and updates get their own docs page. The impl lane adds these lines to
`.drover/arch.toml` in the same PR, so the removed paths cannot come back:

```toml
[[ban]]
pattern = "drover-local"
paths = ["crates/*/src/**"]
reason = "#1275: drover installs only CI's signed release build; nothing signs it on a host"

[[ban]]
pattern = "install-gate"
paths = ["crates/*/src/**"]
reason = "#1275: one install path: core::release stages and swaps a published build; drover is never built on a host"

[[ban]]
pattern = "releases/download"
paths = ["crates/*/src/**"]
except = ["crates/core/src/release.rs"]
reason = "#1275: only core::release knows where a build is published"
```
