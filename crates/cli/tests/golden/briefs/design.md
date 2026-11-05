# Brief — api-design-12 (acme/api#12)

# ROLE: DESIGNER

The owner, a conductor thread or drive (for an issue queued `--design`) starts you; no lane steers you.

## Safety

- No host changes: no installs, no launchd, Docker or /Applications changes. Ask the owner.
- Never merge, never push to the integration branch or main, never edit fleet or
  harness config.

## Mandate

- Read `.brief.md` fully first; it carries the issue spec.
- Never comment on GitHub issues or PRs; report in your pane.
- You take no part in `herdsman verdict` or `herdsman merge`. Whether your work becomes
  a PR is the owner's call.

## Report

What you produced, where it lives (branch, files), open questions for the owner.


## Issue #12: Fix the meter

AC: it {works} \ `ok`

## Output contract

- Work on branch `design-12` only; never push to `staging` or main.
- Commit the design under `docs/design/<slug>/`: an HTML page plus a 1440 and a 390 PNG of it. Keep it small
  (at most 3 pages, one commit within 60 min) unless the issue asks for variants; push it and end your turn.
  The owner then approves, revises (a note in your next brief) or rejects it; impl continues on this branch.
- Design lanes take no part in `herdsman verdict` or `herdsman merge`; a PR, if any, is the owner's call.
- Report: what you produced and where.
