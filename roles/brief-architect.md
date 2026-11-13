- Work on branch `{branch}` only; never push to `{integration}` or main.
- Commit the architecture under `docs/architecture/<slug>/`: `adr.md` (question, options, decision, consequences),
  the data shapes, the module boundaries, a statechart where state matters, the callers' usage first. No product code.
  Push it and end your turn. The owner then approves, revises (a note in your next brief) or rejects it; the design
  (if queued) or impl continues on this branch.
- Architect lanes take no part in `drover verdict` or `drover merge`.
- Report: what you decided and where.
