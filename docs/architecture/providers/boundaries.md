# Providers: module boundaries and state (#806)

## Who may call whom

```
                     ┌──────────────────────────────────────────────────────────────┐
 crates/cli,         │ core (spawn, setup, access, usage, proxy, shed, doctor,       │
 crates/server  ───▶ │ timer, events, hook, config, lanes, orchestrator, hosts, …)  │
                     └───────────────┬──────────────────────────────────────────────┘
                                     │ only `providers::{get, all, lanes, acp_only, accounts, proxied,
                                     │  vendor_of_agent, of_agent, brands, lane_names, known}` and the shapes
                                     ▼
                     ┌──────────────────────────────────────────────────────────────┐
                     │ providers/mod.rs   trait Provider, shapes, registry!, Generic │
                     └───────┬───────────┬──────────┬──────────┬──────────┬─────────┘
                             ▼           ▼          ▼          ▼          ▼
                        claude.rs   hermes.rs   dsh.rs    codex.rs   gemini.rs   (fake.rs: #[cfg(test)])
                        claude/meter.rs
```

Rules (each with its enforcer):

| Rule | Enforcer |
|---|---|
| No `claude`, `anthropic`, `codex`, `openai`, `gemini`, `hermes` (any case, comments too) in `crates/*/src/**` outside `crates/core/src/providers/**` | arch-check `[[ban]]` with `ignore_case = true` (replaces the CI grep step #316) |
| Code outside `providers/` reaches a provider module only through `providers::` (never `providers::claude::…`) | the submodules are private (`mod`, not `pub mod`): the compiler |
| A provider module never calls another provider module | private siblings plus review; nothing they need is shared but `mod.rs` |
| A provider module may call core helpers (`herdr`, `runner`, `lanes::chat`, `access::banner`, `spawn::role_prompt`, `config`) | allowed: providers sit inside core, so no layer edge; core never calls back by name |
| Adding a provider touches only `providers/<name>.rs` and the `registry!` line | `providers/fake.rs` (`#[cfg(test)]`): its tests run real entry points on a provider core has never named |

Out of the ban's paths: `crates/cli/tests/**` (tests), `crates/core/hooks/*.json` (provider data files, read with
`include_str!` from its provider), docs, `web/` (see the ADR's open question).

## Statechart: an `[accounts]` account, as the provider facets feed it

State is already in the event log (`account-capped`, `accounts reset`); this change only moves where the inputs come
from. `cap_banner`/`usage_limited`, `reading` and `signed_out`/`signed_in` are the provider's; every transition and
its event stays core's (`access`, `check`, `usage`).

```
   reading(status line) → Limits: a usage.jsonl row in any state, no transition

            usage_limited(screen) = Some(until); logs account-capped
   ┌──────┐ ─────────────────────────────────────────────────────▶ ┌───────────────┐
   │  ok  │                                                        │ capped(until) │
   └──────┘ ◀───────────────────────────────────────────────────── └───────────────┘
     │  ▲      now ≥ until, or the owner's accounts reset
     │  │
     │  │ signed_in(dir) = Some(true)
     ▼  │
   ┌────────────┐
   │ signed-out │ ◀── signed_out(screen), or signed_in(dir) = Some(false)
   └────────────┘
```

The lane launch (`ready → prepare → start_cmd → startup_prompt? → kick → confirmed`) is unchanged and already per
provider; no new state.
