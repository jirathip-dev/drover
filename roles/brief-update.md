## Update onto {0} (the PR conflicts with or is behind it)

This is a fix round; a fresh review follows at your pushed head.

1. Merge `origin/{0}` into this branch (`git fetch origin && git merge origin/{0}`). No rebase, no force-push.
2. Resolve the conflicts preserving both sides' intent.
3. Run the targeted tests for the files you resolved (never the full gate set; CI runs it, a red CI is a fix round) and push.
4. Report the resolved files.
5. `drover turn done` gates the pushed head (PR body at the head, format, arch-check); a refusal means fix, push, and
   run it again.