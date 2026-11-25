# Report mode (#363)

`POST /report` (a paired device) keeps a bundle as `state/reports/<id>/bundle.json` and emits a `report` event;
`drover report` lists them. The old board's report mode that posted it went with the board (#927).
