# baseline: what git does (#1099)

Run on 2026-11-16 with git 2.56.0 in a scratch clone (`local`, integration `trunk`, baseline `$T`, an older branch
`oldbr` cut two commits before `$T`, lane worktrees `wt1`, `wt2`), and on a scratch copy of this repo's history.

| # | Command | Result | So |
|---|---|---|---|
| 1 | `git replace --graft $T` in `local`, then `git log --oneline \| wc -l` in `local` and in `wt1` | 3 and 3 (9 with `--no-replace-objects`) | a graft is one ref in the shared git dir: every worktree sees it |
| 2 | `git config extensions.worktreeConfig true; git config core.useReplaceRefs false; git -C wt1 config --worktree core.useReplaceRefs true` | `local` 9, `wt1` 3, a worktree on `oldbr` 7; `repositoryformatversion` stays 0 | the view can be per worktree, off by default |
| 3 | `git -C wt1 show --stat <a commit before $T>` | prints it | old objects stay readable |
| 4 | a commit in `wt1`, then `git merge-base HEAD trunk` with and without `--no-replace-objects` | the same sha | a branch cut after the baseline is unaffected |
| 5 | in `wt1` (view on): `git merge-base trunk oldbr`; `git merge oldbr`; `git merge-tree --write-tree trunk oldbr` | exit 1, no output; `fatal: refusing to merge unrelated histories`; exit 128 | a branch cut before the baseline must never be looked at through the graft |
| 5b | the same `merge-base` and `merge-tree` in the worktree with the view off | the fork point; exit 0 | with the view off nothing changed |
| 6 | `git merge --no-ff oldbr` on `trunk`, a fresh lane `wt2` with the view on, `git log --oneline \| wc -l` | 12 of 13 | one graft is undone by the first older branch merged after it |
| 6b | `git --no-replace-objects rev-list --boundary --branches --remotes ^$T`, graft each `-<sha>` line, count again | 8, equal to `rev-list --boundary trunk ^$T \| wc -l`; the fork point shows as a root | grafting the boundary is the cut the issue means |
| 7 | `git replace --graft $T` in a second clone | the same replacement sha in both | deterministic; clones agree without transfer |
| 7b | a commit and `git push origin issue-2` from `wt2` (view on) | exit 0; origin has no `refs/replace`, no replacement object; the commit's real parent is the real tip | pushes and GitHub are untouched |
| 8 | `git blame -L1,1 --porcelain` in `wt2`, then with `--no-replace-objects` | the baseline, marked `boundary`; then the true first commit | blame stops at the baseline; drover's own blame must carry the flag |
| 9 | `git show --stat $T` in `wt2`, then with the flag | the whole tree as added; then its real one-line diff | the baseline reads as a root commit through the view |
| 10 | a branch cut before `$T`, `git merge trunk` in it, grafts caught up, view on | log 11 of 15; `merge-base HEAD trunk` equal with and without the flag | after an update round an older branch is safe to switch on |
| 11 | `git replace -d $(git replace -l)` | `wt2` counts everything again | undo is local and instant |
| 12 | `git -C wt2 --no-replace-objects log`, and `GIT_NO_REPLACE_OBJECTS=1 git -C wt2 log` | both count everything | the flag works after `-C`; one seam in the runner is enough |
| 13 | `git --no-replace-objects replace --graft $T` twice | second: `error: replace ref … already exists`, exit 255 | list first, graft only the missing |

On this repo's history (integration at `f549fe7c8`, 2,867 commits, 1,474 of them merges):

| Baseline | Commits not in its history | Boundary commits to graft | Seen through the view |
|---|---|---|---|
| 50 first-parent commits back (`6e618cd22`, Nov 14) | 348 | 15 | 363 |
| 150 back (`7abb427af`, Nov 10) | 1,017 | 11 | 1,028 |
| 300 back (`3a5047828`, Nov 5) | 1,827 | 16 | 1,843 |

- Grafting the 15 took 0.18 s. All 15 are GitHub-signed merge commits: `git replace --graft` warns
  `the original commit … has a gpg signature` on stderr and exits 0.
- Counting the boundary over every remote branch instead of integration gives 109 grafts for the first row
  (`origin` has 693 branches). The design counts from integration and the lane's own head only.
- `git replace --graft` refuses a commit that merged a signed tag (`mergetag`). None exists here; if one does,
  that worktree stays on full history with `baseline-failed`.

Not measured, from git's documentation: replace refs are ignored by pack transfer, prune and fsck (so nothing
old is ever collected or re-fetched), and `git config --worktree` fails on a clone with several worktrees unless
`extensions.worktreeConfig` is on.
