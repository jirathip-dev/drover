# Workspace roots (#630)

Everything drover makes on a host lives under its workspace root: worktrees at `<root>/<repo>/<lane>`, the
each lane's Cargo build at `<root>/.build/<agent>` (cloned copy-on-write from its fleet's warmest one on the same root, on that host; no reflinks, e.g. ext4: it starts
cold and `build-cloned` logs why) and its
scratch under `<root>/.build/.scratch`.

`roots = ["/Volumes/NVMe/drover", "~/.local/share/drover/work"]` lists several in order (an external drive
first): a new lane takes the first that is mounted, writable and has `min_free_gb`; its spawn logs that `root`, and
an issue's later lanes stay on it while it can take them. `drover hosts eject <host> [<root>]` (default the first)
takes the root out at once: each lane there is saved (`wip/<agent>-<stamp>` for anything unpushed or uncommitted),
its worktree removed, the live one restarted on the next root from that save; then it prints
`safe to unplug <root>` (exit 1 while an agent still works there). A root pulled without eject is noted once by
`check` (`! root <path> gone`): its live lane restarts on the next root from its branch, and the issue's waiting line
says what was lost. Mounted again, the root takes new lanes; nothing moves back on its own. `drover status`, the
lane sheet and each host card (with `/api/v1`'s host data) show the roots, their state (`active`, `ejecting`,
`gone`) and free space.

A fleet's `local` that starts with `~/` points to that path under each host's own home. An absolute `local` is used
as written on every host.
