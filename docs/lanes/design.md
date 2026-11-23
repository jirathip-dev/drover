# Design and architect (#412, #730)

Two roles run before impl, started by the owner, an orchestrator thread or drive, on the main host only. Neither writes
product code; the owner approves, revises (a note in the next brief) or rejects their pushed work, and impl starts
only after approve.

- `design` (UI, `queue add N --design`): the medium the issue asks for. By default HTML mockups on the repo's own
  design system in `docs/design/<slug>/`; an issue naming SwiftUI views in a design path (`apple/design/<slug>/`) gets
  preview-only views and fixtures there (#868), and the hook keeps the lane's edits to `docs/design/` and those paths.
  A PNG of each page or view at the sizes the issue asks for (default 1440 and 390 wide; iPhone and Mac frames for
  native) and a README mapping each PNG to the decision it shows; at most 3 pages unless the issue asks for variants.
  #873: a repo in another medium sets it once in `.drover/design.md` (`medium:`, `folder:`, `render:`, `sizes:`,
  `clip: yes` for a short gif or mp4 beside the images, `tools:` host tags); the issue's own design path overrides it.
  The lane writes only under that folder, and a tool the main host lacks ends its turn `blocked` before it starts.
- `architect` (`[fleet.<n>.role.architect]`, `queue add N --architect`, `queue set N architect on|off`): in
  `docs/architecture/<slug>/` an ADR (question, options, decision, consequences), the data shapes as types or a
  schema, the module boundaries (which module may call which), a statechart where state matters and the callers'
  usage sketched first. With both marks the architect runs first, then the design, then impl. Every later lane's brief
  links the approved ADR; rev checks the diff against it. A shed lane's role label is `architect`.
