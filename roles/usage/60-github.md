## GitHub

The issue is the spec: read it with `drover brief`; PR/CI with `drover pr` / `drover wait` (cached), never
`gh issue view` / `gh pr checks` loops. Never comment on issues or PRs; `drover merge` writes the one closing comment.
UI PRs: confirm the `ui` job is green and open its `ui-390` screenshots. Any visual change not in the issue is a FAIL. Code: write normal code, `cargo fmt` before committing; the formatter decides line breaks, never pack lines to fit the cap.
