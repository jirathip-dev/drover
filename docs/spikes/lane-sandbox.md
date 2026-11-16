# Spike: every lane inside srt (sandbox-runtime), go/no-go (#782)

Decision q1077: wrap each lane's agent in Anthropic's sandbox-runtime (`srt` 0.0.83), Seatbelt on macOS and
bubblewrap on Linux, with srt's own allowlist proxy. No hand-written `sandbox-exec` profile, no provider sandbox mode.

## What ships

- `sandbox = "srt"` in `[fleet.<n>]` (unset = off, the spike default). Any other value is a config error.
- At spawn, drover writes `<worktree>.srt.json` beside the worktree (outside what the lane may write), makes the
  lane's temp folder `$DROVER_SCRATCH_TARGET/tmp`, and starts the harness as
  `env CLAUDE_CODE_TMPDIR=<lane tmp> srt --settings <worktree>.srt.json <harness command>`.
  The pane shell (env, the secrets file it sources) stays outside; the agent, its tools, shells, hooks and MCP
  servers all run inside. Any harness: the wrap is in `Plan::start_cmd`, not in a provider.
- What a harness itself needs comes from its provider (`Provider::sandbox`: folders it reads, writes, hosts it reaches).
- Not covered: shed-acp lanes (their command is built by `acp_cmd`, not `start_cmd`) and orchestrator lanes.
- `srt` must be on the lane host's PATH (`npm i -g @anthropic-ai/sandbox-runtime`). It must not be installed
  under a path the lane may write: srt warns and the lane could rewrite its own sandbox.

## The profile (generated per lane)

Reads are deny-then-allow, writes allow-only, network allow-only:

| | entries |
|---|---|
| denyRead | `~` (all of home), `~/.ssh`, `~/.aws`, `~/.gnupg`, `<config dir>/secrets` |
| allowRead | the worktree, the lane's repo (`local`), `fleet.toml` (only that file of the config folder), drover's binary folder, `~/.cargo`, `~/.rustup`, `~/.local/share/mise`, `~/.local/bin`, `~/.gitconfig`, `~/.config/git`, `~/.config/gh`, `~/Library/Developer`, `~/Library/Keychains`, the agent's profile folder and install, every write path |
| allowWrite | the worktree, `<repo>/.git` (a worktree's objects and refs live there), `CARGO_TARGET_DIR`, `DROVER_SCRATCH_TARGET`, the lane's temp, drover's state folder, `~/.cargo/registry`, `~/.cargo/git`, `~/.npm`, `~/Library/Developer`, `~/Library/Logs/CoreSimulator`, `/private/tmp`, the agent's own folders (`~/.claude`, `~/.claude.json`) |
| network | `github.com`, `*.github.com`, `*.githubusercontent.com`, `crates.io`, `*.crates.io`, `static.rust-lang.org`, `registry.npmjs.org`, plus the provider's (`*.anthropic.com`, `claude.ai`, `*.claude.ai`) |
| mach-lookup | srt's defaults plus `com.apple.CoreSimulator.*`, `com.apple.trustd.agent` |
| unix sockets | under `/private/tmp` |

On the lane's own host every allowed path is also resolved: its real path, the targets of symlinks directly in it,
and each symlinked folder on the way to it (Seatbelt never follows a link for you).

## What broke, and the allowance that fixed it

Found by trial on the Mini (macOS, srt 0.0.83), each run from this lane:

| broke | symptom | allowance |
|---|---|---|
| git | `unable to access ~/.gitconfig: Operation not permitted` (it is a link into a dotfiles repo) | read `~/.gitconfig`, `~/.config/git` and the link targets |
| git commit in a worktree | writes go to the main clone's `.git` | write `<repo>/.git` |
| `drover` in the lane | `command not found`: `~/.local/bin/drover` links to `~/.local/share/drover/bin` | resolve the links in each allowed folder |
| the agent | `claude: command not found` (a link to its versions folder), then its user settings (a dotfiles link) did not load | the provider's install folder, link targets |
| the agent's login | `401 OAuth access token has expired` | read `~/Library/Keychains` (write is not needed) |
| `gh` | `tls: failed to verify certificate: x509: OSStatus -26276` (Go asks trustd) | mach `com.apple.trustd.agent` |
| `simctl` | `Failed to subscribe to notifications from CoreSimulatorService`, log file denied | mach `com.apple.CoreSimulator.*`, write `~/Library/Logs/CoreSimulator` |
| `CARGO_TARGET_DIR`, scratch | `Operation not permitted` though listed: the build root is reached through a symlinked folder under the denied `~` | allow each symlinked folder on the path (it does not open its target's siblings: other worktrees stayed denied) |
| herdsman's own tests | `fake_shed.rs:53 … Operation not permitted`: they bind sockets in `/tmp/hsb-*.sock` (sun_path is too short for the lane temp) | write `/private/tmp` and unix sockets there |
| temp files | srt sets `TMPDIR=/tmp/claude`, shared by every sandboxed lane | `CLAUDE_CODE_TMPDIR=<lane tmp>`, which srt turns into the lane's `TMPDIR` |
| git credential store | `fatal: failed to store: -60008` after a fetch | none: the fetch works, the keychain item is only not rewritten |

## Negative proofs (from inside the sandbox, the generated rules of this lane)

Shell probes under `srt --settings <generated>`:

```
ls ~/.ssh                                → ls: ~/.ssh: Operation not permitted                 exit 1
cat ~/.ssh/known_hosts                   → Operation not permitted                              exit 1
cat ~/.config/drover/cooee-token         → Operation not permitted                              exit 1
ls ~/.config/drover/accounts             → Operation not permitted                              exit 1
cat <config>/secrets/PROBE_KEY           → Operation not permitted                              exit 1
ls <config>/secrets                      → Operation not permitted                              exit 1
ls ~/Projects   (other repos)            → Operation not permitted                              exit 1
ls ~/.local/share/drover/work/drover     → Operation not permitted (other worktrees)            exit 1
touch ~/probe-outside                    → Operation not permitted                              exit 1
touch ~/Projects/herdsman/probe-outside  → Operation not permitted (the lane's own main clone)  exit 1
curl https://example.com                 → curl: (56) CONNECT tunnel failed, response 403       exit 56
curl https://pypi.org                    → curl: (56) CONNECT tunnel failed, response 403       exit 56
```

The secrets probe ran with a scratch `fleet.toml` whose `secrets/` folder sits inside an allowed write path,
the hard case: the more specific `denyRead` still wins. The Mini keeps no `secrets/` folder today.

The same four, run by the agent itself (`claude -p … --dangerously-skip-permissions` under srt, its Bash tool):
`ls ~/.ssh` and `cat ~/.config/drover/cooee-token` → `Operation not permitted`; `touch ~/Projects/herdsman/outside-probe` →
`Operation not permitted`; `curl https://example.com` → `CONNECT tunnel failed, response 403`.

Positive: writes in the worktree, `CARGO_TARGET_DIR` and the lane temp; `git status`, `git fetch`; `gh pr list`;
`drover status`; `xcrun simctl list devices`; `cargo test -p drover --test lane_sandbox --test access` (exit 0).

## Overhead (Mini, warm, medians of 3)

| run | plain | under srt |
|---|---|---|
| `srt -c true` vs `sh -c true` (spawn cost) | 0.01 s | 0.21 s |
| `claude -p "Reply OK"` (2 runs) | 1.94–2.29 s | 2.10–2.61 s |
| `cargo test -p drover --test lane_sandbox --test access` | 0.30 s | 0.52 s |
| the `access` test binary alone | 0.15 s | 0.29 s |
| `xcrun simctl list devices` | 0.25 s | 0.25 s |

The cost is a fixed ~0.2 s per sandboxed process tree (srt's node start and proxy), nothing per syscall worth measuring.

## Not verified here

- Real lanes under srt (impl and rev on herdsman, verify on cooee, an iOS simulator test via hermes-sim-task):
  an impl lane cannot spawn lanes or edit fleet.toml. Needs the owner: `npm i -g @anthropic-ai/sandbox-runtime`,
  `sandbox = "srt"` on one fleet, then one lane of each kind.
- An iOS simulator boot and test run: `simctl` works inside, a boot was not run from a lane on the shared simulator.
- Linux (boat1, bubblewrap): not run.
- Two of four tool-using `claude -p --dangerously-skip-permissions` runs under srt hung before any network call
  (killed at 150 s); the two after them passed in ~7 s. Not isolated; a suspect is an OAuth refresh wanting a
  keychain write. An interactive lane must be watched for it.

## Go / no-go

**Go, behind the switch, for one pilot fleet**, with two conditions before it becomes the default:

1. `/private/tmp` is writable and its sockets reachable only because herdsman's tests bind `/tmp/hsb-*.sock`;
   that also exposes drover's ssh control sockets (`/tmp/drover-ssh-%C`), a way to another host around the proxy.
   Move those tests' sockets (and drover's) off `/tmp`, then drop the allowance.
2. Run the four real lanes above and the Linux host once, and track the hang above.

Accepted trade-offs: `~/.config/gh` is readable (a lane opens PRs, so its token is in reach anyway); the keychain
files are readable (items stay behind securityd's ACLs); trustd is reachable (srt flags it as an exfiltration path,
gh needs it); drover's state folder is writable (lanes log their own events today).
