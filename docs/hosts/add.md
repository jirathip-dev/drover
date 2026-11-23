# Add a host

1. Make the machine an SSH host name, e.g. an entry in `~/.ssh/config`. drover reaches the host as `ssh <name> …`
   for everything (git, probes, file reads and writes, and shed through `shed connect`). Key-based login only, because drover runs
   ssh with `BatchMode=yes` (#337: and `ControlMaster=auto`, one connection kept 10 minutes). The board's reads of a
   host (#337) are one ssh call per step. A pass checks all of a host's
   lane worktrees with one ssh call (#880); a host whose call runs past its limit shows in the tick's log as
   `tick: <host>: over budget; skipped for the rest of this pass`, and the pass goes on for the other hosts.
   #1252: a tick past its `slow_secs` budget skips only its maintenance (clean, trim, lesson, hotspots, compact);
   check and drive still reach every host that answers. Hosts skipped two passes running log one `hosts-skipped`
   alert with the calls not sent and the pass's slowest step.
2. On the new host, install and log in to:
   - shed (its server job running; `hosts bootstrap` installs it);
   - git;
   - `gh` (`gh auth login`): lanes there open their PRs with it;
   - git access to each fleet repo: the host clones each repo itself the first time a lane goes there, with plain
     `git clone https://github.com/<repo>.git <local>`, so its git config (`insteadOf`, credential helper) applies;
   - the agent harness (Claude Code logged in);
   - drover itself. The lane's tool-guard hook calls `drover hook`, and the Claude profile lives under that host's
     own home: each spawn there syncs its lane's profile (`profiles sync --profile <p> --role <r>`, #592).

   drover never copies credentials to another host: a lane on another host runs on that host's own Claude login
   (a `shed-acp` host's, through the main host's token proxy).
3. Add the host to fleet.toml:

```toml
[hosts.mini]            # the main host: no `machine`
tags = ["fast"]

[hosts.bazzite]
machine = "bazzite"     # its SSH name
tags = ["gpu"]          # hand tags, added to the detected ones
root = "~/lanes"        # workspace root on that host (default ~/.local/share/drover/work)
enabled = true          # false: no new lanes go there (see below)
```

#1616: every `drover drive` keeps the main host's shed machines in step with `[hosts]`: each enabled host with a
`machine` shed does not list (`shed machine ls --json`) is added as `shed machine add <machine> <machine>`. drover
never removes a machine from shed, for a host turned off or dropped from `[hosts]` either. `doctor` shows one
`✗ shed machine <machine>` row per missing one, with that command as the fix.
