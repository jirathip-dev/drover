# Facts

Measured on the Mini on 2026-11-18, or read in the code at dc199874d. The last section lists what is not measured.

## Gatekeeper and a bare binary

Run on a notarized, Developer ID signed command-line binary that is already on the Mini (a bare Mach-O, as drover's
asset will be):

```console
$ spctl -a -vv <binary>
<binary>: rejected (the code is valid but does not seem to be an app)
origin=Developer ID Application: <team>
exit 3
$ spctl -a -vv -t install <binary>
<binary>: accepted
source=Notarized Developer ID
exit 0
$ codesign -vv -R=notarized --check-notarization <binary>
<binary>: valid on disk
<binary>: satisfies its Designated Requirement
<binary>: explicit requirement satisfied
exit 0
$ xcrun stapler staple <a copy>
The staple and validate action failed! Error 73.
```

So the AC's plain `spctl -a -vv` can never pass for this kind of file, `-t install` is the assessment that answers
"Notarized Developer ID", and "stapled or recorded" can only be "recorded".

The live drover today:

```console
$ codesign -dv drover
Identifier=drover
CodeDirectory v=20400 size=27103 flags=0x0(none) hashes=842+2 location=embedded
TeamIdentifier=not set
$ ls -lL drover     # 13.2 MB
```

## In the code

| Fact | Where |
|---|---|
| The Mini swaps as soon as its own build is gated; nothing waits for a boat. | `crates/core/src/update.rs`, `install` |
| Each boat is installed by `check`'s survey, in the background, by `cargo install`. | `crates/core/src/hosts.rs`, `survey`, `sync`, `install_line` |
| The guard compares the boat's `--version` to this build's and refuses on any difference; spawn then marks the host sick. | `hosts::acp_guard`; `crates/core/src/spawn.rs` |
| A sick row is read again only hourly or after an install lands, so one refusal holds a boat for up to an hour. | `hosts::survey`, `due` and `landed` |
| A non-ACP remote host with another build only gets a `!` warning today. | `hosts::health` |
| `doctor` takes `--config-only` and `--json`. There is no `--tokens`; only the dry run's text names it. | `crates/core/src/cli.rs`, `crates/core/src/doctor.rs` |
| `doctor --config-only` fails where no fleet.toml is, and a boat has none (#592). | `doctor::doctor`; `docs/hosts.md` |
| Doctor's signature row is in the full doctor, not in `--config-only`, so it is no part of the swap gate. | `doctor::checks` |
| `--version` prints `drover <version> (<40-hex sha>)`; the sha comes from `git rev-parse HEAD` at build time. | `crates/core/src/lib.rs`; `crates/core/build.rs` |
| A new alert kind sets its flag where it is emitted; the kind lists are frozen. | `crates/core/src/events/push.rs` |
| Landing judges integration's tip by every check run on it, and a failed one pauses landing and hunts a culprit. A conclusion other than success, neutral or skipped is failed, cancelled included. | `crates/core/src/land.rs`, `suite`; `crates/core/src/gh.rs`, `check_state` |
| `ci` is the only workflow that runs on a push to main; `pages` also runs on staging. | `.github/workflows/` |
| drover's own binary is built bundled: no system sqlite, no TLS library. | `Cargo.toml` |
| shed is still installed by cargo on a host, so hosts keep a toolchain and the job's PATH keeps cargo. | `crates/core/src/shed.rs`; `crates/core/src/timer.rs` |

## The live fleet

- The repo's default branch is staging (`origin/HEAD`), so a `workflow_run` workflow is live from its merge there.
- The drover fleet sets no `required_checks`; another fleet does.
- The four boats are cloud hosts on `shed-acp`, set up through apt. One more host is configured and disabled.
- The Mini runs bd6ba01, main's head.

## Not measured

- A run started by `workflow_run` reports its checks on the default branch's head commit (GitHub's documented
  `GITHUB_SHA` for that event). If the impl lane finds otherwise, decision 6's "never fails a commit" still holds
  and costs nothing.
- `gh release create` with files uploads to a draft and publishes last. The impl lane confirms it on the first
  staging prerelease; if it does not hold, `publish` creates the draft and publishes it in two steps.
- The boats' glibc version. The stage's version call fails on a boat older than the build image; that host is then
  left behind with the reason in `self-update-failed`.
- How long CI plus the release build takes; `self_update_wait_minutes` (45) assumes about 20 minutes.
