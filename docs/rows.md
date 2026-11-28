# Thread rows (API v1)

Drover serves the things a thread shows as rows: asks, decisions, handoffs, run, config and secret cards, system
lines, owner messages, merges, rollouts and alerts. A row is a pure function of the event log (`core::thread`); there is
no other store. The design is #1571's ADR; this page is the contract as served.

## Routes

The cooee plugin token (`Authorization: Bearer …`) or a paired device reads them; anything else gets `401 UNPAIRED`.

| Route | Answer |
|---|---|
| `GET /api/v1/manifest` | `Manifest`: name, version, `api` (the base URL the client reached), `screens` (the places), `actions`, `destructive` and `threads`, the chat paths (`/cooee/profiles`, `/cooee/thread-started`, attach, detach, rows, stream, a thread's rows and `said`); `cooee-plugin.toml` is `just gen`'s copy of it |
| `GET /api/v1/rows?since=<seq>&n=<1..500>` | `Rows`: the rows after `since`, oldest first; `more` when more follow |
| `GET /api/v1/rows?open=1` | `Rows`: each key's newest row while it is still open, `prompt` and `push` null |
| `GET /api/v1/rows/stream` | server-sent events: `id:` the row's seq, `data:` one row; from `Last-Event-ID`, else from now |
| `GET /api/v1/thread/<id>/rows?before=<seq>&n=<k>` | `Rows` of one thread, the newest `n` before `before`; `more` for older ones |
| `POST /api/v1/thread/<id>/said` | the plugin token only: `Said` (`id`, `ts`, `from: owner`, `text`), an attached chat's owner line logged as `thread-msg` with `item` and `via: client` for `answer --typed`; a repeated `id` logs nothing; `Reply`, `NO_SUCH_THREAD` unless `<id>` is an attached chat |

## A row

`seq`, `ts` (the event it is made from), `key` (a later row with the same key replaces it: a card that settles), `to`
(`{"k": "thread", "id"}`, `{"k": "desk", "repo"}` or `{"k": "owner"}`), `kind` (`ask`, `decision`, `handoff`,
`run-card`, `config-card`, `secret-card`, `system`, `merged`, `rollout`, `digest`, `alert`, `said`), `fleet`, `issue`,
`text` (one plain line), `block` (what every surface draws, in the shared blocks schema), `open`, `prompt` (the line
the thread's agent is told), `push` (`title`, `body`, `tag`; set on every row that notifies, whatever notifiers fleet.toml has), `handoff` (a
handoff row's item) and `epic` (#1786: on a landed epic's `merged` row, keyed `epic-<fleet>-<parent>`, its `title`,
`merged` children with `issue`, `pr` and `title`, and `dropped` issues; else null). A `said` line a client copied has
neither `prompt` nor `push`.

## Where a row goes

An event about a thread (its asks, cards, messages) goes to that thread. Any other row of an issue goes to the issue's
home (`thread::route::home`): the open thread that filed (`issue-filed`) or queued it (the `thread` every event an
orchestrator logs carries), unless that thread's repo is `none`; else `{"k": "desk", "repo": <fleet>}`. An event of no
issue and no thread goes to the owner. The home is read from the log up to the row's seq, so a replay gives the same
target. Drover still delivers to its own threads; it types nothing into an attached chat (`orchestrator-attached`
not followed by `orchestrator-detached`): a lane ask routed there is a `system` row whose `prompt` the client types in.

## The contract

`crates/cli/tests/golden/contract/v1/` holds a recorded answer (`<name>.json`) and its flattened keys (`<name>.keys`,
one sorted `path: type` line each, `k` and `kind` with their value) per route and row kind. `contract_v1.rs` drives the
real server and fails on any difference; `BLESS=1` adds new lines only. `drover arch-check` fails a PR that removes or
changes a line of a `.keys` file (`append_only` in `.drover/arch.toml`): a breaking change is `/api/v2`.
