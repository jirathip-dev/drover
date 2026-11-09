# The lifeboat (#725)

A small fallback page for when cooee is down: from the phone, without a terminal, see the fleet and unblock it.
`herdsman serve` serves it at **`/api/v1/lifeboat`** (e.g. `https://<your serve host>/api/v1/lifeboat`); it needs no
cooee and no board script.

It shows, on one plain page (server-rendered HTML, a few lines of CSS, no JavaScript):

- each fleet, running or paused, with **[pause]** / **[resume]**;
- the queue in order: fleet, issue, title, state, its lane and the age of its last event;
- the live lanes: name, host, state and what each waits on (from the step's snapshot: a page load runs no subprocess, #337);
- the open asks with their options as buttons, and a free-text answer;
- the open run cards (command, why, what it touches, the command's sha) with **[approve]** / **[deny]**.

Each button posts a form and comes back to the page with the result line on top. The actions are the CLI verbs' own code,
with the same refusals: `herdsman answer` (`by: owner`), `herdsman card approve|deny`, `herdsman pause|resume`, each
event logged `via: lifeboat`. It adds no rule and no action of its own; new wants go to cooee.

**Access.** A paired device only (`devices.rs`, the same pairing as every herdsman page), and the request's Host must be
the bind address or a `[serve].allowed_hosts` name. An unpaired device sees only how to pair: tap [pair], run the
`herdsman serve approve <code>` line on the Mac, tap [claim], open the page again. The pairing routes (`/pair/request`,
`/pair/claim`) need no script either, so the lifeboat outlives the old board's removal (#746) as long as they stay.

Its code is `crates/server/src/lifeboat.rs` (routed from `crates/server/src/serve/v1.rs`); the `ui` job audits it at
390 px (`lifeboat-page.png` in the `ui-390` artifact): no sideways scroll, every control at least 44 px tall.
