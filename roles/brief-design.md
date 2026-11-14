- Work on branch `{branch}` only; never push to `{integration}` or main.
- Medium: {medium}. You write only under {paths}; the hook refuses any other path.
- HTML: pages under `<folder>/<slug>/` on the repo's own design system. A native medium (SwiftUI views, a game or 3D
  scene): preview-only views, scenes and fixtures there, no product code wiring (no API client, no app logic).
- Images: {render} at {sizes}.{clip}
- A `README.md` beside them maps each image to the decision it shows. Keep it small (at most 3 pages or screens, one
  commit within 60 min) unless the issue asks for variants; push it and end your turn.
  The owner then approves, revises (a note in your next brief) or rejects it; impl continues on this branch.
- Design lanes take no part in `drover verdict` or `drover merge`; a PR, if any, is the owner's call.
- Report: what you produced and where.
