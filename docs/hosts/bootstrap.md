# Bootstrap, snapshot and the GitHub token (#255)

`drover hosts bootstrap <name>` (owner only; it starts a stopped cloud host first) sets up a fresh host over SSH.
Each step checks first and is skipped when already done, then is read back after its install, so it is safe to run
again. In order:

1. git, gh, just, Claude Code, Rust with a C linker (apt and the official install scripts), then shed at drover's pin
   and its job (on a `shed-acp` host also node and the ACP adapter, below). Other toolchains a fleet needs go in your
   own snapshot.
2. **drover** at this host's exact sha: `cargo install --git <drover repo> --rev <sha>` into the same root as
   here (e.g. `~/.local`).
3. **Logins**, once and by you: for each of Claude and gh that is logged out, bootstrap opens a shed terminal on the
   host (a sign-in on the board) and stops. In the Claude terminal run the login; in the gh terminal
   paste the host's fine-grained token and press Enter (read without echo, never on a command line or in a log). Then
   run bootstrap again. drover never copies a token from this host.
4. **git over HTTPS** with that token (`gh auth setup-git`): never your SSH key.
5. A clone of each fleet's repo the host fits. Logged `host-bootstrap`. #592: no fleet.toml goes there; each spawn
   syncs its lane's profile (a full `profiles sync` runs only where an old fleet.toml is left; one an old bootstrap
   wrote is removed first, #647). A failed step prints its command, exit code and last 20 output lines, and logs them
   in a `host-bootstrap` event.

**The token.** Make one fine-grained token per cloud host, limited to the fleets' repos, with contents and pull
requests write and nothing else (no admin). It lives only on that host. Each health look reads it with one
`gh api -i` there: the Host card shows `github: token · 2 repos · expires in 80d`, and from 14 days before the
expiry the host turns amber (`! github — token expires in 12d`). When gh there can't use its token (expired, revoked
or not logged in yet) the card says `github: unreadable` and the host turns red, so placement skips it. `drover doctor`
shows the same row. `github_token = "external"` in `[hosts.<name>]` turns the look off for a host whose token is issued
outside drover (boat issues its VMs' from outside): no expiry warning and no bootstrap step. The default, `"managed"`,
keeps it.

**Health on every start.** After a cloud host starts and its address is refreshed, drover runs the health checks
above (and the token look) before placement uses it, so an unhealthy host never takes the lane.

**Snapshot.** Once `drover doctor` is all ✓, `drover hosts snapshot <name>` runs a fresh health look and, with no
✗, the recipe's `snapshot` command (logged `cloud-snapshot`). #1127: a snapshot can leave the shed service there
frozen; when shed then does not answer, drover thaws and restarts it once and checks again, and a second failure is
the snapshot's error, naming the host. The recipe's `create_from` template makes a new
sandbox from it that starts ready. Check both against docs.boat.dev.
