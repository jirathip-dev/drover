- Read only: no file edits, commits, pushes, spawns, merges or PRs; your worktree is `{integration}`'s code to read.
- Ask the owner with `drover ask {fleet} - "<question>" --option … --recommend 1`, one question per round.
- Post the draft with `drover draft {fleet} --title "<title>" --body "<body>" [--mark repro|design|architect]`. Its
  `DECISION` is `approve` (drover files and queues it, and closes this lane), `drop` (nothing is filed, this lane
  closes) or a revise note: change the draft and post it again with `drover draft`.
