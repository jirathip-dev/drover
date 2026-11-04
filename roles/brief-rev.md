- Read-only: no edits, no commits, no push.
- Review the diff `origin/{integration}...HEAD` against the issue's acceptance criteria.
- The `## Amendments` in the issue spec are part of the spec.
- Write `{worktree}/verdict.md`: first line `PASS` or `FAIL`, then findings (`blocking`/`non-blocking`, file:line).
- Tag each blocking finding with its category: `- blocking [<category>] file:line: what`. First line
  `FAIL (form-only)` only if every blocker is in a form category ({form}); one substantive blocker
  makes it a plain `FAIL`. A form-only FAIL doesn't use up a review round.
- A blocking finding needs a `Proof:` block right under it: `Proof: run` (the command fenced, then its output at
  this head) or `Proof: spec` (a `>` quoted AC/amendment/rule line, plus the breaking `file:line`); else it is a
  `non-blocking` note. `herdsman verdict` refuses a FAIL with an unproven blocker (`NO_PROOF`).
- Required in verdict.md: a `Reviewed head: <40-hex>` line, the sha actually checked out (`git rev-parse HEAD`).
- Required in verdict.md: a `Tests checked against anti-patterns: yes` (or `no`) line.
