# CI runner VMs (#537)

`[ci_runners]` (owner-set; the hook guards fleet.toml) describes a repo's self-hosted GitHub runners on cloud VMs:

```toml
[ci_runners]
provider = "boat"        # a recipe, like a cloud host's: the built-in boat, or <config>/recipes/<name>.toml
repo = "owner/app"       # the repo the runners register to
label = "app-boat"       # the VM runners' label
size = "large"           # a key of the recipe's [rate_usd_hour]
ttl_hours = 0            # 0: no TTL
snapshots = false
slots = 3                # runner slots per VM
max_vms = 5
usd_day = 30             # the $/day cap: VMs × rate × 24
vms = ["bx_1", "bx_2"]   # the VMs made by hand; `runners add|remove` change the set in the log
service = "ci-runner"    # the VMs' runner service (the default), the recipe's `{service}`
local_label = "app-self"                          # optional: this host's own runners (`retire-local`)
local_stop = "~/ci/ci-runners.sh down --now"      # optional: the command that stops them
```

The recipe's `runner_*` commands (assets/recipes/boat.toml) create, read and delete a VM and run its VM-side steps.
The verbs, each logged `runners-<verb>`; an orchestrator runs them while its `runners` group is on, no other lane may:

- `drover runners status`: the health line (`9/9 online`: online VM runners of `slots × VMs`), each VM's state and
  each runner.
- `runners revive <vm>`: a fresh registration token to that VM and a restart of its runner service.
- `runners add [n]`: n VMs to the spec, each revived. Past `max_vms` or `usd_day` it refuses `RUNNERS_CAP`.
- `runners remove <vm|name>`: drain the VM (no job running, 30 min at most), deregister its runners, delete it; or
  deregister one idle runner by name. The last VM with online runners refuses `RUNNERS_LAST`.
- `runners tokens`: push a fresh token to every VM now. `drover check` (the 5-minute timer) is the refresher: it
  does the same every 30 min.
- `runners retire-local`: run `local_stop`, then deregister the idle `local_label` runners. While VM runners are
  below expected it refuses `RUNNERS_LOW`.

Past a refusal the only way is an owner ask: `drover ask <fleet> "drover runners add 2: ok?" --option yes --option
no --kind owner`; once the owner picks the first option, run the verb again with `--approved <id>` (one yes lets one run on).

The registration token comes from `gh api` on this host and reaches the VM on stdin, never in argv, the log or the
output; the provider key never leaves this host, and no other credential goes to a VM. `check` alerts `runners-low`
once when online VM runners drop below `slots × VMs`, and `runners-ok` once when they are back.
