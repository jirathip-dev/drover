# Auto-update

Every enabled remote host gets the main host's build from the self-update job, staged first and swapped together with
the main host: [releases](../releases.md). Each `check` only reads a host's `drover --version`; when it changed, the host's
health is looked at again so its `acp guard` row clears. `drover hosts` shows each remote host's drover sha, with `✓`
when it matches the main host's.

## When the main host is down

Nothing new starts. Lanes on other hosts keep running in their own shed servers. When the main host comes back,
`check` finds those lanes again from the log. To move the main host, follow docs/move.md.
