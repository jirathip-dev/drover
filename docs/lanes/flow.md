# Typical flow

```sh
drover queue app add 12 13          # ordered queue (mv / rm / block --on N [--on M] / unblock / ls)
drover spawn app 12 impl            # worktree + pane + brief + verified kick
drover spawn app 12 rev             # after the implementer pushed
drover verdict app 12 PASS --sha <head> --file <rev worktree>/verdict.md
drover merge app 12                 # exact-head PASS + green CI + round limit
drover clean --yes                  # reclaim merged or closed-issue lanes (the housekeeping job runs it every 5 minutes)
drover clean --orphans --yes        # also remove folders under the worktree root of no fleet
```
