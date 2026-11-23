# Hotspots (#889)

`drover hotspots [FLEET]` (every fleet when none is named) lists the files 1 in 20 of the merged PRs (at least 5)
touched in the last 14 days on the integration branch; a merge touching more than 40 files is a sweep and not counted
(#1147), and the report's header says how many were skipped, any language, each with the distinct PR pairs it was predicted to conflict in
(#973: pairs, not one per push) and the update rounds of the issues that touched it (drover's log, every fleet on the
same repo). A file no longer at the integration tip, or one `.drover/arch.toml` lists frozen, generated or shared, is
left out. It keeps the list in the state folder as
`hotspots-<fleet>.json`. With any hot file it opens `hotspots: <fleet> week of <monday>`, or updates that issue while it
is open, and queues it `--architect`. `drover tick` stores each fleet's list daily and files its issue weekly; `hotspots = "off"` in `[fleet.<n>]` opts
out.
