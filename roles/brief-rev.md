- Read-only: no edits, no commits, no push.
- Review the diff `origin/{integration}...HEAD` against the issue's acceptance criteria.
- The `## Amendments` in the issue spec are part of the spec.
- Write `{worktree}/verdict.md`: first line `PASS` or `FAIL`, then findings (`blocking`/`non-blocking`, file:line).
- Tag each blocking finding with its category: `- blocking [<category>] file:line: what`. First line
  `FAIL (form-only)` only if every blocker is in a form category ({form}); one substantive blocker
  makes it a plain `FAIL`. A form-only FAIL doesn't use up a review round.
- Required in verdict.md: a `Reviewed head: <40-hex>` line, the sha actually checked out (`git rev-parse HEAD`).
