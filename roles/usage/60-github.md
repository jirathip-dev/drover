## GitHub

The issue is the spec: read it with `drover brief`; PR/CI with `drover pr` / `drover wait` (cached), never
`gh issue view` / `gh pr checks` loops. Never comment on issues or PRs; `drover merge` writes the one closing comment.
UI PRs: confirm the `ui` job is green and open its `ui-390` screenshots. Any visual change not in the issue is a FAIL. Code: write normal code, `cargo fmt` before committing; the formatter decides line breaks, never pack lines to fit the cap.
Verdicts show on the PR as drover's own commit statuses at the exact head: `drover/review` (rev's PASS/FAIL, `pending` at a head not yet reviewed), `drover/verify` (verify's, or why it was skipped) and `drover/override` (an owner override); CI gates ignore them and only drover posts them.
