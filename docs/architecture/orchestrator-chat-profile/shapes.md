# Orchestrator chat profile: data shapes (#1057)

## The seat: where one orchestrator's chat sits

```rust
/// One orchestrator as a cooee chat: who it is, and the fleet its lane identity names.
pub struct Seat<'a> {
    /// `default` for `[orchestrator]`, else the `[orchestrator.<id>]` key.
    pub id: &'a str,
    /// Its `name`, else `Orchestrator`.
    pub name: &'a str,
    /// The lane fleet: `DROVER_LANE`'s, and the fleet its asks are on.
    pub fleet: &'a str,
    /// Every fleet it covers (`fleets`; all of them for `default` or `all`).
    pub fleets: Vec<&'a str>,
    /// That fleet's orchestrator role under `[chat]`, as a new thread's (`defaults`).
    pub role: Role,
}

/// Every orchestrator id: `default`, then each `[orchestrator.<id>]`.
pub fn ids(cfg: &Config) -> impl Iterator<Item = &str>;

/// `NO_ORCHESTRATOR` when none of its fleets has an orchestrator role.
pub fn seat<'a>(cfg: &'a Config, id: &str) -> Result<Seat<'a>>;
```

The lane fleet is picked by one rule, the one an all-repos thread already uses (`role(cfg, "all")`):

| Orchestrator | Lane fleet |
|---|---|
| `[orchestrator]` (`default`), or one whose `fleets` has `all` | the first fleet that has an orchestrator role |
| `[orchestrator.<id>]` | the first of its `fleets`, as written, that has an orchestrator role |

## The served profile

cooee's `Profile` refuses unknown keys, so drover serves only keys it has. Only two constructors build one, so a hidden
profile never carries a guard's paths and an offered one always carries all three.

```rust
/// One entry of `GET /cooee/profiles`.
#[derive(Serialize)]
pub struct ChatProfile {
    name: String,
    label: String,
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<PathBuf>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    why: String,
}

impl ChatProfile {
    fn offered(seat: &Seat, agent: &str, instructions: PathBuf, settings: PathBuf, policy: Option<String>) -> Self;
    fn hidden(id: &str, label: &str, agent: &str, why: String) -> Self;
}
```

| Key | Value | From |
|---|---|---|
| `name` | the orchestrator id | `Seat.id` (cooee shows it as `drover/<id>`) |
| `label` | its name | `Seat.name` |
| `agent` | the role's provider's ACP agent id | `providers::get(harness).acp()`; none: hidden |
| `instructions` | `<state dir>/chat/<id>.md` | written by this request (below) |
| `settings` | what cooee's settings variable takes for that provider | new `Provider::chat_settings(lane profile folder)` |
| `env` | `DROVER_LANE=<fleet>:-:orchestrator`, `DROVER_ORCHESTRATOR=<id>`, `DROVER_THREAD={thread}` | `Seat` |
| `policy` | the lane fleet's `[runtime] acp_policy`, if set | as drover's own ACP lanes |
| `hidden`, `why` | the guard is not proven | `spawn::unguarded(…)`, or no seat, or no ACP agent |

Never served: `account` (cooee's), `folder` (the persona's, else the chat's scratch), `avatar` (the persona's),
`mcp` (cooee's registry is another one), `threads`, the fleet's `env` (its `secret:` names are drover's store, not
cooee's) and `DROVER_PORT` (an orchestrator runs no dev server; one profile cannot hold a port per thread).

## The instructions file

One per orchestrator, at `<state dir>/chat/<id>.md`, inside the folder the hook already keeps agents out of.
`chat::profiles` renders each and writes it only when the text changed (a temp file, then a rename), so the request is
idempotent and a reader never sees half a file.

| Part, in order | Source | Thread brief | Chat brief |
|---|---|---|---|
| Role: who you are, permissions | role part (new file, split from the role prompt) | yes | yes |
| The drover thread | thread part (new file, split from the role prompt) | yes | no |
| The cooee chat | chat part (new file) | no | yes |
| The usage doc | `usage_doc()` | through the lane profile's system prompt | yes |
| Where you sit | thread, repo, fleet for asks / orchestrator, fleets, fleet for asks | yes | yes |
| Instructions, memory, capability groups, skills | `standing(cfg)` | yes | yes, the same call |

The split rule for the role prompt: a sentence goes to the thread part when it names `thread reply`, `thread title`,
`thread show`, `thread handoff`, `--typed`, the `[<thread>] owner:` line framing, `config propose`, `secret request`,
`card post` or the idle sleep. Everything else is the role part. No sentence is in two files.

The chat part says, in a few lines: your reply is the chat message; your thread id is `$DROVER_THREAD`; a choice is
still `drover ask … --thread $DROVER_THREAD` and shows as a card over the chat; no `DECISION` line comes back, so read
`drover asks <fleet> --all` when the owner writes again; `--basis` answers work on your fleet for asks; run cards,
config proposals, secret requests and handoffs need a drover thread.

## The link event

```
orchestrator-attached   fleet = <lane fleet>   {"thread": "<cooee thread id>", "orchestrator": "<id>", "via": "profile"}
```

The thread id passes the rule `attach` has today (at most 128 of letters, digits and `-_.:`). A second
`thread-started` for a thread already linked to that fleet logs nothing.

## Config

`[orchestrator.<id>]` ids are checked at load: lowercase letters, digits and `-`, and not `default` (cooee's profile
name rule; `default` is `[orchestrator]` itself). No new key.
