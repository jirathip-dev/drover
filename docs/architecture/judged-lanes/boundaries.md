# judged-lanes: module boundaries and the statecharts (#1694)

## Who may call whom

```
shed (wire, cli) <── agents (shed's list)  <── spawn, doctor, setup, config checks, hygiene, server (cached)
       ^         <── accounts (#1576)      <── spawn, check, doctor                     (unchanged)
       ^         <── term (run words)      <── spawn, acp::probe
       ^         <── session (events)      ──> acp::audit ──> hook (rules)
cli hook --acp ──> acp (wire, call) ──> hook (rules)
providers (one module's extras) <── spawn, profiles sync, sandbox
```

| Module | Owns | May call | Must not |
|---|---|---|---|
| `core::agents` (new, `agents.rs`) | `Agent`, `Catalog`; the one reader of `shed agents --json` (`shed::adapter` moves in); `fits`, `effort`, `named`, the doctor row | `shed::cli`, `config` | name an agent id; know a lane or a rule; read an account |
| `core::acp` (new, `acp/mod.rs`) | `Request`, `ToolCall`, `Verdict`, `Guard`; `guard`, `ready`, `command`, `answer` | `acp::call`, `hook`, `agents` (types), `providers::of` (has it a hook format) | start a process but through the runner; read a hook-format JSON |
| `acp::call` (new) | ACP `toolCall` to `hook::Call`: the mapping table, nothing else | `hook::Call` | read config, env or the log |
| `acp::probe` (new) | the test run, `Verdict`, the `agent-guard` stamp | `term`, `shed`, `events`, `agents`, `accounts` | run on serve's request path |
| `acp::audit` (new) | asked and unasked calls per lane; `acp-unasked`, `lane-unguarded`; the judged rounds doctor counts | `acp::call`, `hook::decide`, `hook::policy`, `events`, `session::pulse` | call `term`; the follower stops the lane |
| `hook` (`hook.rs`) | `Call`, `Policy`, `Rules`, `decide`, `policy`; every rule | config, events, hygiene, heavy, card, `agents::Catalog::cached` (a co-author trailer's names) | name ACP or a hook format (after the cut-over); know who sent the call |
| `core::session` (`session/turn.rs` new) | shed's events for a lane: state, pulse, and for a judged lane turn end, push, a denied call's reason and the audit's feed | `shed`, `events`, `acp::audit`, `drive::kick_once`, `term` (a prompt, stopping a lane) | judge a call itself |
| `term` (`term.rs`) | the `shed run` words, with the judge's when judged | `shed` | choose the guard |
| `providers` | one module: profile files, session options, sandbox needs, its env lines; until the cut-over its hook format | as today | be asked for install, login, model, effort, vendor, brands or an agent's name: those methods leave the trait |
| `spawn` | the plan and the start | `agents`, `accounts`, `acp` (`guard`, `ready`, `probe::guarded`, `command`), `term`, `providers::of` | read shed's list itself; build judge words |
| server | chips and rows | `agents::Catalog::cached` only | call shed on a request |

The `Provider` trait after slice 5: `sync`, `ready`, `prepare`, `acp_profile`, `acp_meta`, `chat_settings`,
`sandbox`, `profile_files`, `hooks_allowed`, `doctor`, `env`, and until the cut-over `hook_dialect`.
`providers::of(agent_id)` returns the module for that id, else the empty default. Deleted, with their
callers moved to `core::agents`: `Surface`, `lanes`, `acp_only`, `lane_names`, `known`, `default_acp`,
`of_agent`, `vendor_of_agent`, `brands` and the methods `surface`, `vendor`, `brands`, `login`, `install`,
`health_check`, `efforts`, `takes_model`, `model_default`, `acp`, `acp_env`, `sessions`, `process_names`,
`scrubs_env`. `proxy`, `proxied` and `token_env` are #1697's to delete; #1694 neither reads nor moves them.

## One judged call

```mermaid
stateDiagram-v2
    [*] --> Parse: shed runs the judge, the request on stdin
    Parse --> Deny: not JSON, or tool_call null
    Parse --> Map
    Map --> Deny: execute with no command, a write with no path, a mode switch
    Map --> Allow: Free (read, search, think, fetch)
    Map --> Lane: Shell or Write
    Lane --> Deny: no DROVER_LANE, or fleet.toml unreadable
    Lane --> Rules
    Rules --> Deny: a rule refuses (its text is the reason)
    Rules --> Heavy
    Heavy --> Deny: a heavy kind, not under drover heavy (the reason is the line to run)
    Heavy --> Allow
    Allow --> [*]: the verdict allow
    Deny --> Told: shed logs the reason; the follower sends it at the turn's end
    Told --> [*]
```

shed adds its own deny when the judge exits non-zero, prints no verdict or takes over `acp_judge_secs`.

## An agent's guard

```mermaid
stateDiagram-v2
    [*] --> Unknown: a role names it; no stamp for its installed version
    Unknown --> Guarded: it asked before the shell command, drover read the call, no marker file
    Unknown --> Unguarded: a marker file exists
    Unknown --> Unreadable: it asked, the command or path could not be read
    Unknown --> Inconclusive: signed out, silent, or it never tried
    Guarded --> Unknown: shed lists a new version
    Guarded --> Unguarded: lane-unguarded (an unasked call the rules refuse)
    Unguarded --> Unknown: a new version, or doctor runs the test again
    Unreadable --> Unknown: a new version or drover build, or doctor
    Inconclusive --> Unknown: doctor, or the next spawn after the account is signed in
```

Spawn starts a judged lane only in `Guarded`. In `Unknown` it runs the test once, inline, within
`acp_probe_seconds`. In the other three it refuses `ACP_UNGUARDED` at once, with the agent and the stamp's
reason, and spends no turn.

## The cut-over, per fleet

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Hooked: #1694 merged, nothing changes for this fleet
    Hooked --> Both: card B sets acp_policy to judge
    Both --> Hooked: the card is reverted (a miss, or a stuck lane)
    Both --> GateMet: card A's round recorded, 10 judged Claude rounds, 0 unasked, no lane-unguarded
    GateMet --> Judge: the cut-over issue merges (hook format deleted)
    Judge --> [*]
```
