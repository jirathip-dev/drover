This week's triage for `{fleet}`, worked by a `triage` lane (#1100).

1. Read the open issues, the recent merged PRs, the code on the integration branch and the owner's decisions.
2. Close what is done, a duplicate or superseded (`drover triage close`, with the evidence); fold stacked
   amendments into one current spec (`drover triage fold`); label `stale?` what you can't decide on
   (`drover triage label`); file a missing follow-up a closed issue's out-of-scope list names.
3. Docs pass: list each stale, contradicting or bloated passage in the README, `docs/`, `roles/`, `.drover/rules.md`
   and any `CLAUDE.md` / `AGENTS.md` (file:line, what is wrong, the proof), and file them as one `docs:` issue
   (`drover triage docs`); no finding, no issue. Never edit docs, never propose a change to the README's intent.
4. Never touch an issue that is queued, has a lane, waits on an open ask or is labeled `keep` (drover refuses it).
5. End with `drover done --no-change "<one line>" {fleet} <this issue>`: the summary, with an undo for each change,
   goes to the fleet's orchestrator thread.
