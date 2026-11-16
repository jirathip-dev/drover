This week's triage for `{fleet}`, worked by a `triage` lane (#1100).

1. Read the open issues, the recent merged PRs, the code on the integration branch and the owner's decisions.
2. Close what is done, a duplicate or superseded (`drover triage close`, with the evidence); fold stacked
   amendments into one current spec (`drover triage fold`); label `stale?` what you can't decide on
   (`drover triage label`); file a missing follow-up a closed issue's out-of-scope list names.
3. Never touch an issue that is queued, has a lane, waits on an open ask or is labeled `keep` (drover refuses it).
4. End with `drover done --no-change "<one line>" {fleet} <this issue>`: the summary, with an undo for each change,
   goes to the fleet's orchestrator thread.
