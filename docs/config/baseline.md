# History baseline (#1099)

`drover baseline <fleet> [<sha>]` (the owner's; default the integration tip) writes `baseline = "<sha>"` in
`[fleet.<n>]` and logs `baseline-set`; a sha not on the integration branch is `REFUSED BASELINE_OFF_INTEGRATION`.
Lanes whose branch contains it see `git log` and `git blame` only since it, by local grafts; nothing is rewritten.
Remove the key to undo it. How it works and the other undos: `docs/workflow.md`, "History baseline"; the design:
`docs/architecture/baseline/`.
