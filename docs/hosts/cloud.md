# Cloud hosts (#254)

A cloud host is a `[hosts.<name>]` with a `cloud` recipe. drover starts it when a lane needs it and stops it when idle.

```toml
[hosts.boat1]
machine = "boat1"          # the ssh alias drover writes
cloud = "boat"             # the built-in recipe; or <config>/recipes/<name>.toml
size = "small"             # a key of the recipe's [rate_usd_hour]: boat's small|default|large|xlarge
budget_usd_month = 50      # hard: none set means 0, so it never starts
max_hours_day = 6          # optional
idle_stop_minutes = 20     # the default
tags = ["docker"]

[limits]
overflow_after_minutes = 10  # the default
```

- **The recipe** is a TOML file of `sh -c` templates: `create`, `start`, `stop`, `status` (prints `running` when up,
  nothing when there is no such sandbox), `address` (prints `user@host port`, then the host key line), `destroy`.
  A start whose `status` is empty runs `create` (`create_from` once the host was snapshotted) instead of `start`. It also sets `key_env`, the provider key's
  variable, and `[rate_usd_hour]` per size. `{name}` is the host's name and `{size}` its size. The built-in `boat`
  recipe is assets/recipes/boat.toml. Check its commands and rates against docs.boat.dev, and copy it to
  `<config>/recipes/boat.toml` to change them. #635: it targets the current boat CLI (`boat new --type --name`,
  `list --all --json`, `resume`, `stop`, `delete`, `snapshot <id> <name>`, `new --from`; it needs `jq`) and finds the
  sandbox by name. `user@host port` is the sandbox's `sshEndpoint`, the host key comes from `ssh-keyscan` at the first
  start and stays pinned: a start or survey that sees another key fails closed, and `hosts bootstrap <name>` re-pins
  it (logged `cloud-repin` with both keys). `identity` (boat: `~/.ssh/ascii_box_ed25519`, the only key a new sandbox
  takes) adds `IdentityFile` and `IdentitiesOnly yes` to the host block; doctor checks the file exists. `id` prints the
  sandbox's id and `{id}` in a line runs it: boat reads an empty id as the sandbox it runs in, so each recipe line
  refuses an empty one, and drover runs no `stop` or `destroy` when `id` prints nothing. Sizes
  are `small|default|large|xlarge`; an old `size = "medium"` is run as `default` and `drover doctor` shows a `!`
  row for it. doctor also checks each `boat` subcommand the recipe uses against `boat --help` (✗ with the recipe line).
- **The provider key** (#707) is optional: the `boat` CLI owns its own login, so a recipe command runs with no key.
  With `key_env` set, the key is read on the main host from the environment, else the 0600 file
  `<config dir>/secrets/<key_env>`, and each command gets it on stdin and exports it (the `[ci_runners]` API calls use
  it as `$BOAT_API_KEY`). `drover doctor` names the one it found (`✓ cloud boat1 key — env`), or warns `!` with none.
  It never appears in argv, fleet.toml or the event log.
- **Start on demand.** Local Macs come first, even when a cloud host is idle (#256). A running cloud host that admits
  and has `max_working` room takes a lane as soon as every local host refuses or is full (#1152). A stopped one is
  started only when every local host has been refused (busy, low memory or disk) for `overflow_after_minutes`, or the
  lane needs a tag only the cloud host has. The `spawn` event's `why` says which (`HOST_BUSY mini: …; cloud1 is up with
  room`, `overflow: every local host busy 12 min`, `needs docker: no local host has it`), `drover status` shows it as
  `on <host>: <why>`, and `spawn --dry-run` prints it as `on <host>: <why>`. A respawn on a lane's
  own host, `--host`, or a role `host` pin starts it too. `spawn` runs `start`, then polls `status` for up to 5 min.
  It then rewrites the host's block in `~/.ssh/config.d/herdsman` and its own known_hosts file
  (`~/.ssh/herdsman/<name>.known`, outside `config.d`) from `address`, and then spawns. A dry run never starts a host.
  Bootstrap of a running host, and each survey of a cloud host whose `status` says running, reads `address` again and
  rewrites the block when it moved.
  Add `Include config.d/herdsman` at the top of `~/.ssh/config` once: name only that file, since ssh reads every
  included file as config. doctor flags an `Include` that reads a `.known` file (the pre-#635 place); start
  removes those.
- **Idle stop.** `check` stops a running cloud host after `idle_stop_minutes` with no live agent on it. First, each
  lane worktree there with unpushed or dirty work is pushed to `wip/<host>-<lane>`, after the secret scan. A secret
  or a failed push aborts the stop (logged `cloud-abort`). The disk is kept. Lanes there show `paused: cloud host
  stopped` on the Host card, and their next spawn starts the host again.
- **Spend** is the running time (`cloud-start` → `cloud-stop` in the log) × the size's rate, per UTC month and day.
  The Host card shows `spent $12 of $50 · 3.1 h today of 6`, the address age and the last start and stop. The row
  turns amber at 80 % and red at the limit. At the limit, placement refuses `CLOUD_BUDGET` and `check` stops a running
  host (after the wip save). Only an owner edit to fleet.toml raises a limit.
- **Destroy** is owner only: `drover host destroy <name>` does the wip save, then runs the recipe's `destroy`.
