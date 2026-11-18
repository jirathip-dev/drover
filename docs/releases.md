# Releases and updates (#1275)

drover installs only CI's signed release builds, on every host, through one path. Nothing builds drover from source on
a host. The design is in [architecture/self-update-prebuilt](architecture/self-update-prebuilt/README.md).

## The release workflow

`.github/workflows/release.yml` runs after the `ci` workflow ends green on a push to main or staging (`workflow_run`).
It builds `aarch64-apple-darwin` (macOS runner) and `x86_64-unknown-linux-gnu` (the oldest hosted Ubuntu, for a low
glibc floor), then publishes one GitHub release:

| | main | staging |
|---|---|---|
| tag | `build-<sha7>` | `build-staging-<sha7>` |
| kind | release, kept for good | prerelease; the staging run deletes those over 14 days old, tag included |
| assets | `drover-aarch64-apple-darwin`, `drover-x86_64-unknown-linux-gnu`, `SHA256SUMS` | the same |

The macOS binary is signed with the owner's Developer ID Application certificate (hardened runtime, secure timestamp,
identifier `drover`) and notarized with an App Store Connect API key by `scripts/release-sign.sh`, which then checks
it with `codesign -dv`, `codesign -v -R=notarized --check-notarization` and `spctl -a -vv -t install`. A bare binary
cannot carry a stapled ticket, so the notarization is recorded with Apple and checked online. Only the signed binary
is published.

A failed build, signing or publish step ends its job green with an error annotation and publishes nothing, so the
workflow never turns staging red. The failure shows up as no release: `self-update-waiting`. Re-run the workflow run;
the next pass installs.

### Secrets

Set by the owner in the repo's `release` environment (branches main and staging); only the macOS job reads them:

| Name | What |
|---|---|
| `MACOS_CERT_P12` | the Developer ID Application certificate and key, a base64 `.p12` |
| `MACOS_CERT_PASSWORD` | that file's password |
| `ASC_API_KEY_P8` | the App Store Connect API key, a base64 `.p8` |
| `ASC_API_KEY_ID` | its key id |
| `ASC_API_ISSUER_ID` | its issuer id |

## Self-update

`dev.drover.update` runs `drover self-update` every 5 minutes (`drover setup` or `timer install` writes it). It
fetches `[self_update] branch` (default `main`) of the drover fleet and reads that head's release (`SHA256SUMS`, one
`gh release download`). The hosts are the main host plus every `[hosts]` entry with a `machine`.

1. **No release yet:** nothing changes and nothing is built. The pass prints that it waits, and logs
   `self-update-waiting` once per head when the release is still missing `[limits] self_update_wait_minutes` (45)
   after the head was first seen (`self-update-seen`).
2. **Stage:** each reachable host that is not on the head downloads its asset to `drover.new` beside its live binary,
   checks it against `SHA256SUMS`, checks that `drover.new --version` names the head's sha and, where a fleet.toml is,
   runs `drover.new doctor --config-only`. Any failure removes `drover.new`, keeps the live binary and alerts
   `self-update-failed` once per host and head. A host needs `curl` and `sha256sum` or `shasum`.
3. **Swap together:** when every waited-for host is staged, the same pass logs `self-update-swapping`, swaps each
   remote host (`mv drover.new drover`, the PATH links, its `profiles sync`, logged `host-sync`), then the main host
   last (`self-updated`), runs `profiles sync` and hands serve over: the running serve starts the new binary on its
   listening sockets, drains and execs it in place. A serve that does not answer on `<state dir>/serve-<port>.sock`
   is restarted instead.
4. **A lagging host:** a disabled host, or a cloud host that is not running, is not waited for. An enabled host that is
   unreachable or whose stage fails is waited for up to `[limits] self_update_lag_minutes` (30) after the release was
   first found (`self-update-staging`); then the rest swap and it gets one `host-behind`. Until a later pass stages and
   swaps it alone (or a cloud start installs this build), placement passes over it as `HOST_UPDATING`; its running
   lanes finish. If the main host's own stage fails, nobody swaps.

Until the swap every host runs the old build, so the ACP guard always matches. During the swap's seconds placement
passes over every remote host (`[limits] self_update_swap_secs`, 120, if no `self-updated` follows). A lane spawn
that still finds another build there is refused `HOST_UPDATING`, which is a hold like `HOST_BUSY`: the host is not
marked sick. `drover hosts` shows each remote host's drover sha, with `✓` when it matches the main host's.

`drover self-update --dry-run` prints each host's build and what the pass would do; `--pause` / `--resume` toggle it
(logged). #1031: the doctor gate also says `hello` to the local `shed serve`; a server on another shed protocol fails
it, so the old drover stays and the alert goes out once per mismatch.

`[self_update] staging_serve = true` (off by default) installs the staging head's `build-staging-<sha7>` the same way
into `<state dir>/update-staging/bin/drover` and runs it as `dev.drover.serve-staging`: `serve --read-only` on
`staging_addr` (default `127.0.0.1:7787`).

`hosts bootstrap` installs this build's release on a new host the same way.

## A manual install

Download the asset for the machine from the release of the sha you want, check it, and put it in place:

```sh
t=build-<sha7>; a=drover-aarch64-apple-darwin   # or drover-x86_64-unknown-linux-gnu
gh release download "$t" -R <owner>/<repo> -p "$a" -p SHA256SUMS
grep " $a\$" SHA256SUMS | shasum -a 256 -c -
b=~/.local/share/drover/bin; mkdir -p "$b"; chmod 755 "$a" && mv "$a" "$b/drover.new" && mv "$b/drover.new" "$b/drover"
```

Switching the main host from a locally signed build to the Developer ID build asks once per Keychain item to allow
the new signature.
