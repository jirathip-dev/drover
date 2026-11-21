Decided: the look sheet's style (terminal, ascii, pixel) picks how figure() draws every row, kept per phone, restored 1:1 from c34235855~1
Because: q2001
Decided: each figure carries `act`, Queue and lane figures `grow` and `issue`, Events figures a `moment`, lanes a `coat`
Because: "each row's state, action and stage come in the screen blocks (`crates/server/src/screens.rs`)"
Decided: the main host's load level rides in its Hosts row figure (`load`), from weather(), shared with Scene
Because: crates/server/src/screens_hosts.rs:7
Rejected: a top-level `load` key on the row — blocks' Row is closed, a new fault in the demo ratchet
Decided: the art is the pinned swag pack under assets/swag (SOURCE commit, SHA256SUMS checked by the themes test); no new art
Because: "The art comes from the pinned swag commit, checked against SHA256SUMS."
Decided: Scene rows kept exactly as staging has them after the #1305 merge; the ascii avatar keeps the old 10 px as a ui_tokens exception
Because: "Scene (the full-screen old Herd) is #1305. This issue doesn't touch it."
Check: switch style in [⚙]: Queue, Events, Lanes rows and the Hosts sheet repaint in place; terminal matches today; reduced motion stops loops
Owner: the board's look sheet gets terminal, ascii and pixel again, with the old board's animated sprites, plants, host weather and moments
