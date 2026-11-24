# Hygiene (#1101)

One history and issue convention is on for every fleet with no config. `[hygiene]` sets it for all fleets,
`[fleet.<n>.hygiene]` per fleet (its keys win); `"off"` opts a key out.

| key | default | what |
|---|---|---|
| `merge` | `"squash"` | the merge method `[fleet.<n>] merge` and the repo should use; also `"merge"` |
| `comments` | `"strict"` | `.drover/arch.toml`'s `[comments] policy`; the rule ships in `rules/shared.md` |
| `baseline` | `"setup"` | the arch-check baseline, set at fleet setup, redone on the owner's ask |
| `triage` | `"weekly"` | a weekly triage lane |
| `docs_trust` | `["docs/archive/"]` | paths a new lane worktree leaves out (sparse checkout); `[]` = off |
| `issue_spec` | `"triage"` | triage folds amendments into `## Spec (current)` |
| `docs` | `"on"` | `.drover/arch.toml` has the #735 `[docs]` stale-doc check |

`drover setup` applies the set to each fleet's local clone, writing only what is missing: `[comments] policy = "strict"`
and a `[docs]` section (paths `docs/**/*.md` and `README.md`, the repo's `cli`, `config` and `api` files as `watch`) in
`.drover/arch.toml`, then the first arch-check baseline when `baseline = "setup"` and none exists. Commit what it wrote.

`drover doctor` prints each fleet's `hygiene` line and a red `! hygiene` row for each drift: the fleet's own
`merge` mode (#1097) differing from it, the repo on GitHub not allowing that method, or `comments = "strict"` with integration's `.drover/arch.toml` saying otherwise, or `docs = "on"` with no `[docs]` there.
