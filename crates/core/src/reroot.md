
## Compact: a repo's fresh start

Compact is the standalone `compact` binary (jirathip-dev/compact; `cargo install --locked --git
https://github.com/jirathip-dev/compact`): one root commit before a cut, one commit per landed PR after it, an archive
in `<repo>-history`, and undo. Its README is the reference. Drover keeps only the glue:

- **Run.** `drover compact <fleet> <act>` runs `compact --repo <repo> --line <integration> --clone <clone> --lane
  [host:]<lane folder>… --fleet <each fleet on the repo> --saved <dir>/saved.tsv --dir <dir> <act>`, where `<dir>` is
  `<state dir>/compact/<repo>` and the act is compact's (`--cut <date|sha> [--dry-run | --thread <id>]`, `--undo
  [--dry-run | --thread <id>]`, `--reset`). A lane runs only `--dry-run`.
- **Cards.** With `--thread <id>`, compact posts its own run cards (`drover card post`): `compact … --push <new tip>`
  for every ref but `main` and, when `main` moves, `compact … --main --push <main's new tip>`. Each runs only inside
  its approved card. Pause the fleet first.
- **Records.** The log is never edited. Drover lists its saved commit ids (`reroot::SAVED`; a sha-named field it
  leaves alone is in `reroot::KEPT` with why) in `saved.tsv`. After the push, compact writes `moved.json`, and after
  an undo `undone`; the next `drover compact` or drive pass logs them once as `compacted` (one per fleet, one per
  issue with ids in both histories) and `compact-undone`. Those bridge the two histories for `drover why` and
  hotspots, and drive, spawn and merge refuse `OLD_ROOT` for a clone or PR head that is not on the new root.
- **Guard.** CI's `every_saved_sha_field_is_in_compacts_list` fails a PR that writes a new `*_sha` field in neither
  list (crates/cli/tests/compact.rs).
