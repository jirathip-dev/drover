# Merge label (#947)

`merge_label` (unset by default) is the PR label the full CI needs. drover adds it on a PASS at the head and removes it
when the head moves past that PASS. `drover setup` creates it in the repo, and `drover doctor` checks that it exists.
New fleets set `merge_label = "merge-ready"` with the affected-only CI recipe in [ci.md](../ci.md).
