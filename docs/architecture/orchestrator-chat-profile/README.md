# Orchestrator chat profile: each orchestrator served to cooee as a profile (#1057)

## Summary

Decision: drover serves one cooee profile per orchestrator, named by its orchestrator id. Its instructions file is
rebuilt on each read from the thread brief's parts, with a chat part in place of the thread part; the chat runs under
the orchestrator hook and is linked to drover when it starts.
Slices: the role text split in three parts; the profile list and the chat link in core; two cooee routes with tests.
Open questions: (1) approve a board-fix line on #1057 for three lines in the frozen board router file? (2) run cards,
config proposals and secret requests refuse a cooee chat today: a follow-up issue (my pick), or built here?

Status: proposed (architect lane, 2026-11-15). Read on staging `60ff82fb7`, cooee staging `9280915d6`.

Read this page first (the callers), then [adr.md](adr.md), [shapes.md](shapes.md), [boundaries.md](boundaries.md).

Two words mean two things here. A **lane profile** is drover's own folder of provider settings for a role
(`drover profiles sync` writes it; the orchestrator's holds the hook). A **chat profile** is what cooee is served:
one entry of `GET /cooee/profiles`.

## The callers, sketched first

### cooee asks for the profiles (as its sidebar loads, and again as each thread starts)

```
GET /cooee/profiles            Authorization: Bearer <plugin token>

200 {"profiles": [
  {"name": "default", "label": "Conductor", "agent": "<the role's ACP agent>",
   "instructions": "/…/state/drover/chat/default.md",
   "settings": "/…/config/drover/<provider>/lane-orchestrator",
   "env": {"DROVER_LANE": "herdsman:-:orchestrator", "DROVER_ORCHESTRATOR": "default", "DROVER_THREAD": "{thread}"}},
  {"name": "ops", "label": "Ops", "agent": "<the role's ACP agent>", "hidden": true,
   "why": "profile lane-orchestrator has no drover PreToolUse hook; run `drover profiles sync`"}
]}
```

cooee reads `instructions` from disk and sends it ahead of the first message, puts `settings` in the provider's
settings variable, swaps `{thread}` for its thread id, and refuses to start a `hidden` profile. Then it tells drover:

```
POST /cooee/thread-started     {"thread": "<cooee thread id>", "profile": "default"}
200 {"line": "<thread> is default's chat on herdsman"}
```

### The owner's persona file (cooee's side, its paired issue)

```toml
name = "Conductor"
role = "orchestrator"
fleets = ["cooee", "herdsman"]
avatar = "…"
profile = "drover/default"      # no `character`, no `instructions`: the text is drover's
```

### The server route file (all of it)

```rust
impl Server {
    pub(super) fn cooee(&self, ctx: &Ctx, head: &str, (method, p): (&str, &str), body: &str) -> Res {
        if !Self::plugin(ctx, head) {
            return line("401 Unauthorized", "the plugin token only");
        }
        let cfg = match Config::kept(ctx) {
            Ok(c) => c,
            Err(e) => return line("500 Internal Server Error", &shown(&e)),
        };
        match (method, p) {
            ("GET", "/cooee/profiles") => out("200 OK", &json!({"profiles": chat::profiles(ctx, &cfg)})),
            ("POST", _) if self.addr.is_none() => line("405 Method Not Allowed", "read-only: GET only"),
            ("POST", "/cooee/thread-started") => {
                let s: Started = serde_json::from_str(body).unwrap_or_default();
                done(chat::started(ctx, &cfg, &s.profile, &s.thread))
            }
            _ => line("404 Not Found", "no such cooee route"),
        }
    }
}
```

### Core: the two briefs come from one builder

```rust
let text = brief::thread(cfg, t, fleet);

let seat = chat::seat(cfg, id)?;
let text = brief::chat(cfg, &seat);
```

`brief::thread` is today's `profile()` moved, its output unchanged. Both end with the same `standing(cfg)`: the
owner's instructions file, memory, capability groups and skills index.

### The tests (the issue's AC, by entry point)

```rust
let list = get(&s, "/cooee/profiles");
assert_eq!(names(&list), ["default", "ops"]);
assert!(read(&list[0]["instructions"]).contains(OWN_TEXT));
std::fs::write(&own, "changed");
assert!(read(&get(&s, "/cooee/profiles")[0]["instructions"]).contains("changed"));

assert!(list[0]["hidden"] == true);
run(&c, &r, &["profiles", "sync"]);
let p = &get(&s, "/cooee/profiles")[0];
assert!(hook_of(&p["settings"]).ends_with("hook --role orchestrator"));
assert!(hook_with(env_of(p, "th1"), bash("gh pr merge 5")).contains("deny"));
```
