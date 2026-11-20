Decided: merge origin/staging and rebuild the conflicted web/dist/board.js from the merged web/src with npm run build
Because: "don't let docs/workflow.md, web/dist or the brief goldens drift from `just gen`"
Rejected: hand-merging the minified bundle — it is generated, a hand merge can drift from web/src
Owner: this round only brings the branch up to date with staging; the typing fix is unchanged
