# judged-lanes: data shapes (#1694)

## Role config

```rust
pub struct Role {
    pub agent: Option<String>,   // a shed agent id; required once `harness` is gone
    pub harness: String,         // until the cut-over only: "claude" spells agent = "claude-acp"
    pub model: Option<String>,
    pub effort: Option<String>,
    …                            // profile, mcp, prompt, needs, host: unchanged
}
```

`Config::parse` stays pure: it checks only that a role names an agent. What needs shed's list (the agent
exists, takes that model or effort, `rev` on another vendor than `impl`) is checked where shed can be
asked: spawn, `drover set`, `drover config propose` and doctor.

`[runtime] acp_policy` gains `"judge"`. `[limits]` gains `judge_secs` (30) and `probe_seconds` (120).
At the cut-over `harness`, `[runtime] acp` and `acp_policy` are deleted: every lane is judged.

## What is read from shed's list

`core::agents` is the only reader of `shed agents --json`. Each field is one of shed's.

```rust
pub struct Agent {
    pub id: String,                    // id                       "antigravity-acp"
    pub vendor: String,                // provider.agent           the program it fronts; rev-is-not-impl compares it
    pub installed: Option<Installed>,  // installed.version, .cmd  None: not installed on this host
    pub install: Option<String>,       // provider.install         the fix line of "not installed"
    pub login: Option<String>,         // provider.login           setup's list, the signed-out fix line
    pub status: Option<SignIn>,        // provider.status, status_read
    pub config_dir_var: Option<String>,// provider.config_dir_var  accounts
    pub scrub: Vec<String>,            // provider.scrub           variables a lane's shell unsets
    pub takes_model: bool,             // model_env or model_args is set
    pub efforts: Vec<String>,          // provider.efforts         empty: it takes none
    pub models: Vec<(String, String)>, // provider.models          the model chips
    pub sessions: Vec<String>,         // default_folder + session_files: the leak check's session paths
}
pub struct Installed { pub version: String, pub cmd: Vec<String> }
pub struct SignIn { pub cmd: String, pub key: Option<String> }   // key: the JSON field that says signed in; none: exit 0

pub struct Catalog(Vec<Agent>);
impl Catalog {
    pub fn on(on: &hosts::On) -> Result<Catalog>;   // asks that host's shed: spawn, doctor, tick
    pub fn cached(ctx: &Ctx) -> Catalog;            // the main host's last read, a state file: serve, chips
    pub fn get(&self, id: &str) -> Option<&Agent>;
}
```

Not read by drover, because shed applies them itself: `model_env`, `model_args`, `effort_args`, `env`
(through `shed run --model --effort`), `limit`, `resets`, `auth_failures` (shed reports `rate-limited`
and `error auth:`, which `core::session` already reads).

## The guard

```rust
pub enum Guard {
    Judge,    // shed's judge calls the hook's ACP mode
    Hooked,   // the agent's own hook format calls `drover hook`: today's path
    Both,     // judged, with the hook format still on: the cut-over's live rounds
}
```

| The agent's module has a hook format | `acp_policy` | Guard |
|---|---|---|
| no (every agent but one) | any | `Judge` |
| yes | not `judge` | `Hooked` |
| yes | `judge` | `Both` |

`Hooked`, `Both` and the type go at the cut-over.

## The judge's wire (shed's, from shed#359)

```rust
#[derive(Deserialize)]
pub struct Request {
    pub name: String,                        // the lane's shed name
    pub labels: BTreeMap<String, String>,
    pub cwd: PathBuf,                        // the worktree
    pub tool_call: Option<ToolCall>,         // ACP's toolCall as the agent sent it
    pub options: Vec<Offered>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub tool_call_id: String,
    pub title: Option<String>,
    pub kind: Option<Kind>,
    #[serde(default)]
    pub locations: Vec<Location>,            // { path }
    pub raw_input: Option<Value>,
}
pub enum Kind { Read, Edit, Delete, Move, Search, Execute, Think, Fetch, SwitchMode, Other }

#[derive(Serialize)]
pub struct Verdict { pub decision: Decision, pub reason: String }   // one line on stdout, exit 0
pub enum Decision { Allow, Deny }
```

The judge runs as a child of shed's bridge, so it does not see what the lane's shell exports. The lane's
plain variables (`DROVER_LANE`, `DROVER_HOST`, `DROVER_RULES`, `DROVER_SCRATCH_TARGET`, `DROVER_THREAD`,
the fleet's non-secret `env`) therefore go in `shed run --env`: the judge and the agent see one
environment, and the hook reads its lane as it does today. Secrets stay in the sourced file.

## The ACP-to-hook mapping

The rules judge one typed call, whoever sent it:

```rust
pub enum Call {
    Shell { cmd: String, cwd: String },
    Write { files: Vec<String> },   // every path an edit, delete or move touches
    Mode,                           // the agent asks to change its permission mode
    Free,                           // nothing the rules look at
}
pub fn decide(call: &Call, p: &Policy, git: &dyn Fn(&str, &[&str]) -> Option<String>) -> Option<String>;
```

| ACP `kind` | Read from | `Call` | Then |
|---|---|---|---|
| `execute` | `rawInput.command`, `cmd` or `script`: a string; or an argv list (`sh -c <script>` gives the script, else the words joined). cwd: `rawInput.cwd` or `workdir` when absolute, else the request's `cwd` | `Shell` | every shell rule: `deny_commands`, `HOST_STRESS`, `OPAQUE_SHELL`, `gh`, `git push`, reviewer git, notes, scratch target, drover verbs, protected paths, the cap file, the public-text check |
| `execute`, no readable command | | none | deny, `HOOK_FAILED_CLOSED` |
| `edit`, `delete`, `move` | `locations[].path`, `rawInput` path fields (`file_path`, `path`, `notebook_path`, `filePath`, `absolute_path`, `destination`), the files a patch text names | `Write` | every write rule: the cap file, protected paths, a design lane's folders, a reviewer's read-only |
| `edit`, `delete`, `move`, no path | | none | deny, `HOOK_FAILED_CLOSED` |
| `switch_mode` | | `Mode` | deny, `MODE_SWITCH`: a lane keeps the mode it started in |
| `read`, `search`, `think`, `fetch` | | `Free` | allow |
| `other`, or no kind | a command field: as `execute`; a path field or location: as `edit`; else `Free` | | |
| `tool_call` is null | | none | deny, `HOOK_FAILED_CLOSED` |

Heavy commands: the judge cannot change a call. A `Shell` call that matches a heavy kind and is not
already under `drover heavy` is denied once with `REFUSED HEAVY_SLOT: run it as: <the wrapped line>`. A
call already under `drover heavy … -- <cmd>` is judged on `<cmd>`.

Until the cut-over the hook format's parser builds the same `Call` (`tool_name` and `tool_input` to
`Shell` or `Write`), so `decide` has one body and both entries share every rule.

## The test run's stamp, and the new events

No new state file: the stamp is the latest `agent-guard` event for a host and agent id.

```json
{"kind": "agent-guard", "host": "main", "acp": "antigravity-acp", "version": "1.5.2", "verdict": "guarded"}
{"kind": "agent-guard", "host": "main", "acp": "x-acp", "version": "0.3.0", "verdict": "unguarded", "why": "ran a shell command without asking"}
{"kind": "judge-unasked", "agent": "myapp-rev-12", "acp": "antigravity-acp", "n": 3}
{"kind": "lane-unguarded", "agent": "myapp-rev-12", "acp": "antigravity-acp", "tool": "git push origin staging", "rule": "pushing to `staging` is blocked…"}
```

```rust
pub enum Verdict {            // judge::probe
    Guarded,
    Unguarded(String),        // a marker file exists: it ran a shell command, or wrote a file, without asking
    Unreadable(String),       // it asked, but drover could not read the command or the path
    Inconclusive(String),     // signed out, no reply in probe_seconds, or it never tried the shell command
}
```

- `spawn` gains `"acp": "<agent id>"` and `"guard": "judge" | "hooked" | "both"`.
- `judge-unasked`: at a turn's end, how many `execute`, `edit`, `delete` or `move` calls ran with no
  permission request. Informational; doctor sums it per agent.
- `lane-unguarded`: one of those calls is one the rules refuse. The lane is stopped (its work saved as
  `wip/`), an `agent-guard` `unguarded` is logged for that host, and the owner gets one alert.
- `hook-denied` and `hook-failed-closed` are today's, from the same code.
