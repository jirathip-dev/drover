- Work on branch `{branch}` only; never push to `{integration}` or main.
- Medium: {medium}. You write only under {paths}; the hook refuses any other path.
- HTML (the default): pages under `docs/design/<slug>/` on the repo's own design system. A native medium the issue
  names (SwiftUI views in `apple/design/…`): preview-only views and fixtures in the path it names, no product code
  wiring (no API client, no app logic).
- A PNG of each page or view at the sizes the issue asks for (default 1440 and 390 wide; iPhone and Mac frames for
  native), and a `README.md` beside them mapping each PNG to the decision it shows. Keep it small (at most 3 pages or
  screens, one commit within 60 min) unless the issue asks for variants; push it and end your turn.
  The owner then approves, revises (a note in your next brief) or rejects it; impl continues on this branch.
- Design lanes take no part in `herdsman verdict` or `herdsman merge`; a PR, if any, is the owner's call.
- Report: what you produced and where.
