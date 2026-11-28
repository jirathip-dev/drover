
- Blocks: this repo uses Blocks, so draw every screen with Blocks' renderer on the pinned swag pack ({pack}); no
  hand-written CSS and no ad hoc tokens.
- Stand-ins: anything Blocks cannot draw yet is a stand-in, visibly marked as one in the image, listed in the README
  under `Stand-ins:` and filed in jirathip-dev/blocks, one issue per missing piece, linked from that list.
- Run `blocks check` on the design screens before you hand off and put its result in the README; drover runs
  `just guard` on your pushed head and sends new findings back to you once before the owner sees the design.