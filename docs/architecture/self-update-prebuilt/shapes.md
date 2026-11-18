# Shapes

## A release

| | main | staging |
|---|---|---|
| tag | `build-<sha7>` | `build-staging-<sha7>` |
| kind | release, kept for good | prerelease, deleted with its tag after 14 days |
| assets | `drover-aarch64-apple-darwin`, `drover-x86_64-unknown-linux-gnu`, `SHA256SUMS` | the same |

The binaries are bare executables, not archives. `SHA256SUMS` is `sha256sum`'s format: one `<64 hex>  <asset name>`
line per binary. A host's asset is picked from its probed tags: `os:macos` and `arch:arm64`, or `os:linux` and
`arch:x86_64`; any other pair has no asset.

## The release workflow

```yaml
# .github/workflows/release.yml
name: release
on:
  workflow_run:
    workflows: [ci]
    types: [completed]
    branches: [main, staging]
permissions:
  contents: read
jobs:
  linux:      # ubuntu-22.04; if: the ci run's event is push and its conclusion is success
  macos:      # macos-latest; environment: release; the only job that names a secret
  publish:    # needs: [linux, macos]; permissions: contents: write; runs only with both artifacts
```

Each job checks out `github.event.workflow_run.head_sha` and builds
`cargo build --release --locked -p drover --target <triple>`. The macOS job then runs one script:

```sh
# scripts/release-sign.sh FILE: sign, notarize, verify; the job's only use of the secrets
codesign --force --options runtime --timestamp --identifier drover --sign "Developer ID Application" "$f"
ditto -c -k "$f" "$f.zip" && xcrun notarytool submit "$f.zip" --key "$key" --key-id "$id" --issuer "$issuer" --wait
codesign -dv "$f" && codesign -v -R=notarized --check-notarization "$f" && spctl -a -vv -t install "$f"
```

A step that fails ends its job green with an `::error::` annotation and uploads no artifact (`adr.md`, decision 6).
`publish` writes `SHA256SUMS`, runs one `gh release create <tag> --target <sha>` (with `--prerelease` for staging),
and for staging deletes every `build-staging-*` prerelease created over 14 days ago with
`gh release delete --cleanup-tag`.

Secrets, set by the owner in the `release` environment (or as repo secrets, open question 1):

| Name | What |
|---|---|
| `MACOS_CERT_P12` | the Developer ID Application certificate and key, a base64 `.p12` |
| `MACOS_CERT_PASSWORD` | that file's password |
| `ASC_API_KEY_P8` | the App Store Connect API key, a base64 `.p8` |
| `ASC_API_KEY_ID` | its key id |
| `ASC_API_ISSUER_ID` | its issuer id |

## The install path

```rust
// crates/core/src/release.rs: knows runners and shell lines; no events, no Config, no rollout
pub struct Release { pub sha: String, pub tag: String, sums: BTreeMap<String, String> }
pub enum Found { Release(Release), Missing(String) }
pub struct Asset { pub url: String, pub sha256: String }
pub struct Builds { pub live: Option<String>, pub staged: Option<String> }
pub enum Unstaged { NoAsset(String), Download(String), Checksum, Version(String), Doctor(String) }

pub fn tag(branch: &str, sha: &str) -> String;
pub fn find(r: &dyn Runner, repo: &str, branch: &str, sha: &str) -> Found;
impl Release { pub fn asset(&self, repo: &str, tags: &[String]) -> Option<Asset>; }
pub fn builds(on: &dyn Runner, exe: &str) -> Result<Builds>;
pub fn stage_line(a: &Asset, exe: &str) -> String;
pub fn swap_line(exe: &str) -> String;
pub fn stage(on: &dyn Runner, exe: &str, rel: &Release, a: &Asset, gate: bool, limit: Duration) -> Result<(), Unstaged>;
```

- `find` is one `gh release download <tag> -p SHA256SUMS -O -`. `Missing` carries why: no such release, or `gh`
  failed. Both mean wait.
- `builds` is one call on the host printing the full sha of `exe --version` and of `exe.new --version`; `Err` means
  the host did not answer. A sha is the 40 hex in `drover <version> (<sha>)`.
- `stage` runs `stage_line` under `limit`, compares the printed version to `rel.sha`, and with `gate` runs
  `exe.new doctor --config-only`. On any `Unstaged` it removes `exe.new`.
- `on` is the Mini's own runner or a `hosts::On`; `release` only sees `Runner`.

## The rollout

```rust
// crates/core/src/update.rs
enum Kind { Live, StagingServe }
struct Target<'a> { branch: &'a str, kind: Kind, members: Vec<Member<'a>> }
struct Member<'a> {
    name: &'a str,
    on: On<'a>,
    exe: String,
    tags: Vec<String>,
    main: bool,
    waited: bool,
    gate: bool,
    builds: Option<Builds>,
    staged: Result<(), Unstaged>,
}
struct Marks { seen: Option<u64>, staging: Option<u64>, waiting: bool }
enum Step {
    Current,
    Wait { alert: bool },
    Hold { missing: Vec<String>, until: u64 },
    Stuck,
    Swap { ready: Vec<String>, behind: Vec<String> },
}
fn step(now: u64, head: &str, found: &Found, members: &[Member], marks: &Marks, limits: &Limits) -> Step;
```

- `Kind::Live`: `[self_update] branch`, the live binary, members the Mini plus every `[hosts]` entry with a
  machine. `Kind::StagingServe`: branch `staging`, the binary under `update-staging`, the Mini alone; after its
  swap the staging serve job restarts instead of `profiles sync` and the handover.
- `waited`: enabled, and not a cloud host that is stopped or starting. `gate`: a fleet.toml is there. `builds` is
  `None` for a host that did not answer.
- `step`, in order: every member's live build is the head: `Current`. No release: `Wait`, with `alert` once the
  head has been seen for `self_update_wait_minutes` and `waiting` is not yet logged. The Mini needs the head and is
  not staged: `Stuck`. The Mini is on the head already: `Swap` of whoever is staged, at once (catch-up). Else
  every waited member staged, or `self_update_lag_minutes` since `staging`: `Swap`, `ready` ending with the Mini,
  `behind` the rest. Else `Hold`.
- The swap: log `self-update-swapping`; for each remote member in `ready` one call running `swap_line`, the PATH
  links and its `profiles sync` lines as today, logged `host-sync`; then the Mini's rename; `self-updated`; one
  `host-behind` per member in `behind`; then `profiles sync`, the serve handover and `timer::heal` as today. A
  remote swap that fails moves that host to `behind`; the Mini still swaps.

## Placement and the guard

```rust
// crates/core/src/hosts.rs
pub enum Unguarded { Build { theirs: String, mine: String }, Hook(String), Adapter(String) }
pub fn acp_guard(ctx: &Ctx, on: &On, home: &str, lane: (&str, &str, &str)) -> Result<(), Unguarded>;
pub fn updating(events: &[Event], name: &str, now: u64) -> Option<String>;
```

`updating` reads only the log, for remote hosts and `Kind::Live` events: a `self-update-swapping` newer than every
`self-updated` and younger than `self_update_swap_secs`, or a `host-behind` for `name` newer than its last ok
`host-sync`. `HOST_UPDATING` joins `HOST_BUSY` wherever host holds are listed (placement's busy codes, the start
holds, the waiting words); it is not in `owner_refusal`.

## Events

| Kind | Data | Logged | Phone |
|---|---|---|---|
| `self-update-seen` | `sha`, `branch`, `target` | once per head, when a pass first sees it | no, passive |
| `self-update-waiting` | `key` (the sha), `sha`, `branch`, `target`, `reason` | once per head, after the wait limit | yes |
| `self-update-staging` | `sha`, `branch`, `target` | once per head, when its release is first found | no, passive |
| `self-update-failed` | `key` (`<host> <sha7>`, or the shed line), `host`, `sha`, `reason` | once per key | yes, as today |
| `self-update-swapping` | `sha`, `target`, `hosts` | before the first swap of a rollout | no, passive |
| `host-sync` | `host`, `from`, `to`, `ok` | each remote swap; `check` alerts `host-sync-failed` as today | as today |
| `host-behind` | `host`, `has`, `wants`, `reason` | once per host and head | no, passive |
| `self-updated` | `sha`, `branch`, `target`, `hosts`, `behind` | after the Mini's swap | as today |

`target` is `live` or `staging-serve`. New kinds set `Event::push` or `Event::passive` where they are emitted; the
frozen lists are not edited.

## Limits

| Key | Default | What it times |
|---|---|---|
| `self_update_wait_minutes` | `45` | a head with no release this long alerts `self-update-waiting` |
| `self_update_lag_minutes` | `30` | how long a rollout waits for an enabled host that is not staged before the rest swap |
| `self_update_stage_secs` | `300` | one host's download and gate |
| `self_update_swap_secs` | `120` | how long a `self-update-swapping` mark holds remote placement when no `self-updated` follows |
