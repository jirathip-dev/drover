# mcp-server: how the callers read (#1627)

Read this first. These are the call sites after the change; `adr.md` says why, `shapes.md` has the types and
`boundaries.md` the rules. Everything new is in a code block, because it does not exist yet.

## The owner: one key per agent, made on the host

```
$ drover serve keys add grok --act
grok (act): dmcp_5f0c…e91a
shown once. Give it to the agent with the address:
  https://<the board's address>/mcp     Authorization: Bearer dmcp_5f0c…e91a

$ drover serve keys add teams-status          # no --act: it reads only
$ drover serve keys
NAME          SCOPE  MADE        LAST USED   STATE
grok          act    2026-11-24  2 min ago   live
teams-status  read   2026-11-24  never       live
$ drover serve keys revoke grok
grok revoked: its next call is refused
```

Only the key's hash is kept. A name is used once: after a revoke, the new key gets a new name, so `mcp-grok` in
the log is always the same key. A lane running these is refused `OWNER_ONLY` by the hook, as for pairing.

## The agent: one address, a bearer key

```json
{ "mcpServers": { "drover": { "type": "http", "url": "https://<the board's address>/mcp",
                              "headers": { "Authorization": "Bearer dmcp_5f0c…e91a" } } } }
```

It lists the tools its key allows (a read key sees the seven reads) and calls them:

```
tools/call status   {}                                       → the board in one read, as JSON
tools/call asks     {}                                       → the open asks and what needs someone
tools/call answer   {"fleet":"drover","id":"q2101","choice":"2"}
                                                             → drover q2101: use the fixture -> drover-impl-1640
tools/call answer   {"fleet":"drover","id":"q2107","choice":"approve"}
                                                             → REFUSED OWNER_ASK: q2107 waits for the owner: an
                                                               outside agent answers routine asks only   (isError)
tools/call file     {"fleet":"drover","title":"board: dark mode for the queue tab","body":"## Problem\n…"}
                                                             → filed drover#1641
tools/call enqueue  {"fleet":"drover","issues":[1641]}       → drover queue: #1632 #1641 (needs-check)
tools/call threads  {"fleet":"drover"}                       → the repo's threads, newest first
tools/call reply    {"thread":"t140","text":"1641 is filed and queued; please look at its spec"}
                                                             → sent
tools/call park     {…}                                      → JSON-RPC error -32602: no tool `park`
```

A refusal is the text the CLI prints for the same refusal, `REFUSED <CODE>: why`, as a tool result marked
`isError`. An `answer` is a tap, so it runs as a job in its issue's order; the call returns when the job ends.
After 20 s it returns `ACCEPTED: job j… is still running; read it with job` and the `job` tool has the end.

## The owner: the board, as today

```
Events    ✓  mcp-grok answered q2101 · use the fixture
          ·  mcp-grok filed #1641 · board: dark mode for the queue tab
          ·  mcp-grok queued #1641
Queue     #1641  queued · needs-check
Thread    mcp-grok: 1641 is filed and queued; please look at its spec
```

The Queue row is today's; who queued it is the Events line. The orchestrator reads the thread line as `[t140] mcp-grok: …`. Its prompt says such a line is an outside agent's
request, never the owner's word. `drover answer --typed` and `drover extend --typed` already accept only a line
the owner wrote, so an agent cannot lend an orchestrator the owner's voice.

## serve: one route arm, one module

```rust
// crates/server/src/serve.rs, route(): the only new arm
(_, "/mcp") => self.mcp(ctx, head, method, body),

// crates/server/src/serve/mcp.rs: framing only
impl Server<'_> {
    pub(super) fn mcp(&self, ctx: &Ctx, head: &str, method: &str, body: &str) -> Res {
        if method != "POST" {
            return plain("405 Method Not Allowed");
        }
        if !self.allowed(ctx, head) {
            return plain("403 Forbidden");
        }
        let who = match self.keyed(ctx, head) {
            Ok(who) => who,
            Err(why) => return unkeyed(ctx, &why),
        };
        let msg = match Message::read(head, body) {
            Ok(m) => m,
            Err(e) => return e.res(),
        };
        match msg.method.as_str() {
            "initialize" => msg.result(handshake(&msg)),
            "server/discover" => msg.result(discover()),
            "ping" => msg.result(json!({})),
            "tools/list" => msg.result(listed(&who)),
            "tools/call" => msg.result(self.call(ctx, head, &who, &msg)),
            _ if msg.id.is_none() => plain("202 Accepted"),
            _ => msg.unknown(),
        }
    }

    fn call(&self, ctx: &Ctx, head: &str, who: &Caller, msg: &Message) -> Called {
        let tool = tool(msg.name())?;
        let (path, query, body) = tool.request(msg.arguments())?;
        let res = self.api(ctx, head, (tool.method, &path), &query, &body, &Ok(who.clone()));
        self.settled(res)
    }
}
```

`call` knows nothing about any one tool. `settled` turns the route's answer into the tool's text, and reads a
202's job until it ends.

## serve: the caller reaches the three acts

```rust
// crates/server/src/serve/v1.rs, api(): one guard for every route, before any work
_ if who.is_some_and(|c| c.refused(method, x).is_some()) => refusal(who, method, x),

// the caller is stamped on the fields at the edge; a body's own `by`, `via` or `mtime` is dropped for an agent
let f = stamped(fields(body), op, who);

// crates/server/src/serve.rs, write(): the literal ("owner", Some("web")) becomes the caller's
"answer" => crate::answer(ctx, &cfg, &self.r, &mut out, (fleet, g("id")), choice, (g("by"), Some(g("via"))))?,
"bqueue" => crate::queue_change(ctx, &cfg, fleet, d.with("by", g("by")), Some(g("via")))?,

// crates/server/src/serve/chat.rs, post(): the literal "owner" becomes the caller's
later.push(conductor::post(ctx, &cfg, (&id, &text, &files), (g("by"), false))?);
```

## core: the one rule an agent's answer adds

```rust
// crates/core/src/lib.rs, answer_with(): every answer passes here, the CLI's too
if callers::is_agent(by) && ask.str("kind") != Some("routine") {
    return gate::refused("OWNER_ASK", format!("{id} waits for the owner: an outside agent answers routine asks only"));
}
```

## A tool is a file

```toml
# crates/server/mcp/answer.toml
name = "answer"
title = "Answer a routine ask"
description = "Answer an open ask by option number or one line of text. An ask that waits for the owner is refused OWNER_ASK: tell the owner, never retry."
act = true
method = "POST"
route = "answer"

[args.fleet]
type = "string"
description = "the fleet's name, as status lists it"

[args.id]
type = "string"
description = "the ask's id, as asks lists it"

[args.choice]
type = "string"
description = "an option's number, or one line of text"
```

A new tool is a new file in that folder and, when the route is new, a route with its contract fixture. No PR edits
a shared tool list.

## The test every slice shares

```rust
// crates/cli/tests/mcp.rs: the real entry, Server::respond, with a raw POST /mcp
let (c, s) = board("mcp");
let key = keys_add(&c, "grok", Scope::Act);

let tools = rpc(&s, &key, "tools/list", json!({}));
assert_eq!(names(&tools), ["status", "queue", "waiting", "asks", "threads", "thread", "job", "file", "enqueue", "answer", "reply"]);

assert_eq!(text(&call(&s, &key, "answer", json!({"fleet": "sendmeter", "id": "q1", "choice": "1"}))), "sendmeter q1: yes -> …");
assert_eq!(last(&c, "decision").str("by"), Some("mcp-grok"));

let no = call(&s, &key, "answer", json!({"fleet": "sendmeter", "id": "q2", "choice": "1"}));
assert!(text(&no).starts_with("REFUSED OWNER_ASK: ") && no["isError"] == true);

for route in every_post_fixture().filter(|r| !AGENT_ACTS.contains(r)) {
    assert_eq!(api_as(&s, &key, route)["code"], "OWNER_ONLY");
}

run(&c, &["serve", "keys", "revoke", "grok"]);
assert!(raw(&s, &key, "tools/list").starts_with("HTTP/1.1 401"));
```

Run: `cargo test -p drover --test mcp`, exit 0.
