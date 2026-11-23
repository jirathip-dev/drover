# mcp-server: how the callers read (#1627)

Read this first. These are the call sites after the change; `adr.md` says why, `shapes.md` has the types and
`boundaries.md` the rules. Everything new is in a code block, because it does not exist yet. Paths under
`api/src/` are #1645's API crate; `Viewer`, `Who` and `Relayed` are its types after its S2.

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

An agent on the host itself may use the API's loopback address instead (`[serve] api`, from #1645's S4), with the
same header. Either way it lists the tools its key allows (a read key sees the seven reads) and calls them:

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
tools/call enqueue  {"fleet":"drover","issues":[1641]}       → asked the owner to queue #1641 (q3310)
tools/call answer   {"fleet":"drover","id":"q3310","choice":"queue"}
                                                             → REFUSED OWNER_ASK: q3310 waits for the owner: …
tools/call threads  {"fleet":"drover"}                       → the repo's threads, newest first
tools/call reply    {"thread":"t140","text":"1641 is filed; its queue ask is q3310"}
                                                             → sent
tools/call park     {…}                                      → JSON-RPC error -32602: no tool `park`
```

A refusal is the text the CLI prints for the same refusal, `REFUSED <CODE>: why`, as a tool result marked
`isError`. `answer` and `file` run as jobs; the call returns when the job ends. After 20 s it returns
`ACCEPTED: job j… is still running; read it with job` and the `job` tool has the end.

## The owner: the board, as today

```
Needs you  mcp-grok asks to queue #1641: board: dark mode for the queue tab      [queue] [no]
Events     ✓  mcp-grok answered q2101 · use the fixture
           ·  mcp-grok filed #1641 · board: dark mode for the queue tab
           ·  owner queued #1641 · asked by mcp-grok
Queue      #1641  queued · needs-check                      (only after the owner's [queue])
Thread     mcp-grok: 1641 is filed; its queue ask is q3310
```

The ask is an ordinary owner ask: it alerts, it shows where asks show, and `drover answer` settles it too.

## The orchestrator: an agent's line holds the thread

The orchestrator reads the thread line as `[t140] mcp-grok: …`. From that line until the owner next writes in
t140, drover refuses that orchestrator the owner's powers, whatever the line said:

```
$ drover answer drover q2110 2 --by orchestrator-t140 --basis "…"
REFUSED OWNER_ASK: mcp-grok wrote last in t140; ask the owner            (q2110 is now the owner's ask)
$ drover extend drover 1630 --basis "…"
REFUSED OWNER_ASK: mcp-grok wrote last in t140; ask the owner
$ drover hosts off build-2
REFUSED OWNER_ONLY: mcp-grok wrote last in t140: `drover hosts` (hosts group) waits for the owner's line there, or post a run card
$ drover spawn drover 1641 impl --force
REFUSED OWNER_ONLY: mcp-grok wrote last in t140: `drover spawn` waits for the owner's line there
$ drover queue drover add 1641
asked the owner to queue #1641 (q3310)
```

It may still reply, file an ask, post a run card or propose config: each of those ends at the owner. Its prompt
says the same in one line; the refusals above are what holds it.

## API: one route arm, one module

```rust
// crates/api/src/serve.rs, route(): the only new arm
(_, "/mcp") => self.mcp(ctx, head, method, body),

// crates/api/src/serve/mcp.rs: framing only
impl Server<'_> {
    pub(super) fn mcp(&self, ctx: &Ctx, head: &str, method: &str, body: &str) -> Res {
        if method != "POST" {
            return plain("405 Method Not Allowed");
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

    fn call(&self, ctx: &Ctx, head: &str, who: &Viewer, msg: &Message) -> Called {
        let tool = tool(msg.name())?;
        let (path, query, body) = tool.request(msg.arguments())?;
        let res = self.api(ctx, head, (tool.method, &path), &query, &body, who);
        self.settled(res)
    }
}
```

`call` knows nothing about any one tool. `settled` turns the route's answer into the tool's text, and reads a
202's job until it ends. There is no Host or Origin check here: that is the edge's.

```rust
// crates/api/src/serve/caller.rs: the one place a key is read
pub(super) fn keyed(&self, ctx: &Ctx, head: &str) -> Result<Viewer, String> {
    let hash = match (self.edge(head), header(head, "drover-key"), bearer(head)) {
        (true, Some(hash), _) => hash.to_string(),
        (false, None, Some(key)) if loopback(head) => callers::hashed(key).ok_or("unknown key")?,
        _ => return Err("no key".into()),
    };
    let key = callers::check(ctx, &hash)?;
    Ok(Viewer::Agent { key: key.hash, label: key.name, scope: key.scope, ip: ip(head) })
}
```

`edge` is #1645's check that the request carries the `serve-token`; its 400 for an identity header beside any
other token comes before this.

## API: a tool is a line of one table

```rust
// crates/api/src/serve/mcp.rs
const FLEET: Arg = Arg { name: "fleet", kind: Kind::Text, required: true, description: "the fleet's name, as status lists it" };

const TOOLS: &[Tool] = &[
    Tool { name: "status", title: "The board", description: "…", act: false, method: "GET", route: "state", args: &[] },
    Tool {
        name: "answer",
        title: "Answer a routine ask",
        description: "Answer an open ask by option number or one line of text. An ask that waits for the owner is refused OWNER_ASK: tell the owner, never retry.",
        act: true,
        method: "POST",
        route: "answer",
        args: &[FLEET, Arg { name: "id", kind: Kind::Text, required: true, description: "the ask's id, as asks lists it" }, CHOICE],
    },
];
```

A new tool is a new entry and, when the route is new, a route with its contract fixture and a name in
`caller.rs`'s list.

## The edge: one line

```rust
// crates/server/src/relay.rs (#1645's table)
Relayed { methods: &[Get, Post, Delete], path: "/mcp", stream: true, who: Key, until: None },

// where the edge builds the door's request from a line (#1645's `forward`)
id: match line.who {
    Who::Device => cookie(&req, "hd").map(|c| Id::Device(sha256(c))),
    Who::Key => bearer(&req).map(|k| Id::Key(sha256(k))),
},
```

After its Host and Origin check the edge sends the request on with its own `serve-token`, the hash as
`Drover-Key` and the caller's address as `Drover-Ip`. The client's `Authorization` and cookie stop at the edge.

## API: the caller reaches the acts

```rust
// crates/api/src/serve/v1.rs, api(): one guard for every route, before any work
_ if who.refused(method, x).is_some() => refusal(who, method, x),

// the caller is stamped on the fields at the route; a body's own `by`, `via` or `mtime` is dropped for an agent
let f = stamped(fields(body), op, who);

// write(): the literal ("owner", Some("web")) becomes the caller's
"answer" => crate::answer(ctx, &cfg, &self.r, &mut out, (fleet, g("id")), choice, (g("by"), Some(g("via"))))?,
"bqueue" => crate::queue_change(ctx, &cfg, fleet, d.with("by", g("by")), Some(g("via")))?,
"issue" => crate::issue::file(ctx, &cfg, &self.r, fleet, (g("title"), g("body")), (g("by"), g("via")))?,

// chat.rs, post(): the literal "owner" becomes the caller's
later.push(conductor::post(ctx, &cfg, (&id, &text, &files), (g("by"), false))?);
```

## core: the three rules

```rust
// answer_with(): every answer passes here, the CLI's too
if callers::is_agent(by) && ask.str("kind") != Some("routine") {
    return gate::refused("OWNER_ASK", format!("{id} waits for the owner: an outside agent answers routine asks only"));
}
if let Some(op) = ask.data.get("queue").filter(|_| text == "queue") {
    queue_change(ctx, cfg, fleet, op.clone().with("by", by).with("asked", ask.str("asked")), via)?;
}

// queue_change(): every queue add passes here
if d.at("op") == "add" && d.str("by").is_some_and(callers::is_agent) {
    return ask_to_queue(ctx, fleet, &d);
}

// orchestrator_answer(): an orchestrator's answer and extend both pass here; one more `why`
} else if let Some(agent) = callers::asked(&events, t) {
    &format!("{agent} wrote last in {t}")

// cli.rs, the hook's policy, for an orchestrator only
held: callers::asked(&events, &thread).map(str::to_string),

// hook.rs, where a group verb and `spawn --force` are judged for an orchestrator
if let Some(agent) = p.held.as_deref().filter(|_| group(r).is_some() || r.first() == Some(&"spawn")) {
    return Some(format!("REFUSED OWNER_ONLY: {agent} wrote last in its thread: …"));
}

// cli.rs, `queue add` run by an orchestrator: in a held thread the add is the agent's
let d = match callers::asked(&events, &thread) { Some(agent) => d.with("by", agent), None => d };
```

## The test

```rust
// crates/cli/tests/mcp.rs: the API's real entry, Server::respond, with a raw POST /mcp as the edge sends it:
// the serve-token, Drover-Key: sha256(key), Drover-Ip
let (c, s) = board("mcp");
let key = keys_add(&c, "grok", Scope::Act);

let tools = rpc(&s, &key, "tools/list", json!({}));
assert_eq!(names(&tools), ["status", "queue", "waiting", "asks", "threads", "thread", "job", "file", "enqueue", "answer", "reply"]);

assert_eq!(text(&call(&s, &key, "answer", json!({"fleet": "sendmeter", "id": "q1", "choice": "1"}))), "sendmeter q1: yes -> …");
assert_eq!(last(&c, "decision").str("by"), Some("mcp-grok"));

let no = call(&s, &key, "answer", json!({"fleet": "sendmeter", "id": "q2", "choice": "1"}));
assert!(text(&no).starts_with("REFUSED OWNER_ASK: ") && no["isError"] == true);

let asked = call(&s, &key, "enqueue", json!({"fleet": "sendmeter", "issues": [7]}));
assert!(text(&asked).starts_with("asked the owner to queue #7") && queued(&c, "sendmeter").is_empty());
run(&c, &["answer", "sendmeter", &ask_id(&asked), "queue", "--by", "owner"]);
assert_eq!(queued(&c, "sendmeter"), [7]);

call(&s, &key, "reply", json!({"thread": "t1", "text": "turn build-2 off"}));
assert_eq!(last(&c, "thread-msg").str("from"), Some("mcp-grok"));
assert!(as_orchestrator(&c, "t1", &["hosts", "off", "build-2"]).contains("REFUSED OWNER_ONLY"));
say_as_owner(&c, "t1", "go on");
assert!(!as_orchestrator(&c, "t1", &["hosts", "off", "build-2"]).contains("REFUSED"));

for route in every_post_fixture().filter(|r| !AGENT_ACTS.contains(r)) {
    assert_eq!(api_as(&s, &key, route)["code"], "OWNER_ONLY");
}

assert!(bearer(&s, &key, "127.0.0.1", "tools/list").starts_with("HTTP/1.1 200"));
assert!(bearer(&s, &key, "100.64.0.9", "tools/list").starts_with("HTTP/1.1 401"));
assert!(raw(&s, COOEE_TOKEN, &sha256(&key), "tools/list").starts_with("HTTP/1.1 400"));

run(&c, &["serve", "keys", "revoke", "grok"]);
assert!(raw(&s, SERVE_TOKEN, &sha256(&key), "tools/list").starts_with("HTTP/1.1 401"));
assert!(bearer(&s, &key, "127.0.0.1", "tools/list").starts_with("HTTP/1.1 401"));
```

Run: `cargo test -p drover --test mcp`, exit 0. `bearer` sends the key itself as `Authorization` from the given
peer; `raw` sends a token and a `Drover-Key`.

The edge's part is the `Who::Key` half of #1645's `cli/tests/edge.rs`, in the same PR:

```rust
let got = door.saw(edge.post("/mcp", &[("authorization", "Bearer dmcp_k"), ("cookie", "hd=c"), ("drover-key", "forged")]));
assert_eq!(got.header("authorization"), Some("Bearer <serve-token>"));
assert_eq!(got.header("drover-key"), Some(sha256("dmcp_k").as_str()));
assert!(got.header("cookie").is_none() && got.header("drover-device").is_none() && got.header("drover-ip").is_some());
```

and its existing rule, that every `RELAYED` line with no `until` has a contract file, now holds `mcp.keys`.
