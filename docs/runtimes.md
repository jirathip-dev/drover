# Terminal runtimes: herdr or berth (#298)

A lane is a coding agent running in a terminal. herdsman starts that terminal, types into it, reads its screen and
closes it through one adapter, `crates/core/src/herdr.rs`. There are two runtimes behind it:

- **herdr** (the default): panes in herdr workspaces, as always.
- **berth** (a separate project): a per-user server that keeps
  terminals alive, reads their screens and reports their state.

```toml
[runtime]
kind = "berth"            # "herdr" (default) or "berth"
# socket = "~/x/berth.sock"   # default: berth's per-user socket
# acp = "claude-acp"          # ACP mode: run this installed ACP agent instead of the harness's terminal UI
# acp_policy = "ask"          # ACP permission requests: ask (default), allow or deny
```

With no `[runtime]` nothing changes. The kind decides where **new** lanes start. Lanes already running stay where
they are and are still reached: a herdr pane id always has a colon (`w4:p1`) and a berth terminal name never does.
An agent named in a call (`agent prompt`, `agent wait`) goes to berth only if berth runs a terminal of that name.

## How herdsman talks to berth

herdsman depends on `berth-proto`, berth's wire types crate (serde only, no I/O). It is a git dependency pinned to
berth's merge commit `498c9f0`; berth has no release tag yet, so switch the pin to a tag once one exists. Each call
is one connection to berth's Unix socket: a `hello` line (the versions must match exactly, else `BERTH_VERSION`),
then one JSON-RPC request line, then one answer line. There is no daemon and no async.

On another host (`[hosts.<name>]` with a `machine`, docs/hosts.md) the same lines go through
`ssh <machine> berth connect --autostart`, one ssh call per request, over the host's shared ssh connection.

herdsman keeps speaking in herdr's commands. The berth runtime answers each one the way herdr would, with herdr's
JSON shapes and error codes (`pane_not_found`, `timeout`), so the rest of herdsman needs no change:

| herdsman does          | herdr                        | berth                                                   |
| ---------------------- | ---------------------------- | ------------------------------------------------------- |
| start a lane           | worktree, pane, `agent start` | `git worktree add`, `run` (name = the lane's agent)    |
| read the screen        | `pane read`                  | `read` (`history` + `lines` for the recent window)      |
| type a prompt          | `agent prompt` + Enter       | `send` + `keys enter`                                   |
| press a key            | `pane send-keys`             | `keys` (`Escape` becomes `esc`)                         |
| wait for the agent     | `agent wait --until`         | `wait --until` (reported states)                        |
| wait for screen output | `pane wait-output`           | `wait --output`                                         |
| close a lane           | `pane close`                 | `kill`, then `rm` (so the next lane can use the name)   |
| list lanes             | `agent list`                 | `ls`, keeping running terminals labelled `fleet`        |

A berth lane's terminal is labelled `fleet`, `issue`, `role` and `harness`, kept (`keep`) across a berth server
restart, and runs the user's login shell (`$SHELL -l`, in berth's environment). herdsman then types the lane's
exports and the harness command (`exec <harness> <args>`) into that shell, as it types them into a herdr pane. So no
token is ever in the terminal's command line, in `ps` or in berth's state file. `exec` makes the harness the
terminal's program, so the terminal ends when the harness exits, and the lane drops out of the agent list (lane-dead,
as with herdr).

`herdsman spawn --dry-run` shows the berth requests it would send (`berth <method> <params>`).

## State is reported, not scraped

In a berth terminal `$BERTH` names the terminal. The lane's hook (`herdsman hook`, the same one that guards tools)
then reports the turn's state with berth's `state` method:

- `working` when a prompt is submitted or a tool is used (Claude Code `UserPromptSubmit`, `PreToolUse`,
  `PostToolUse`; Hermes `pre_tool_call`, `post_tool_call`);
- `blocked` when the harness waits on a person (Claude Code `Notification`);
- `done` at a turn's end (Claude Code `Stop`; Hermes `on_session_end`).

`herdsman profiles sync` adds the `UserPromptSubmit` and `Notification` hooks to the Claude profiles. A failed report
is printed on stderr and never blocks the tool call. The kick's confirmation (`wait --until working blocked`) and the
board's agent status read these states.

## drive and berth events

Each drive pass (not the fast step) reads berth's `events` stream since the last `seq` it saw (kept in `berth.seq`
beside the event log), stopping once the stream is quiet for 0.3 s. Every lane's `state`, `exited` and `killed`
event is logged as a passive `lane-state` event (`agent`, `state`, `note` or `code`, `berth_seq`). Only this host's
server is read. When it does not answer, drive prints `berth events: …` and carries on: the agent list it polls each
pass still covers every lane, here and on other hosts. berth keeps only its last 1,000 events, so a long gap loses
the older ones, and the poll still sees the current state.

## ACP mode

With `acp = "<agent id>"` a lane runs that ACP agent (install it with `berth agents add <id>` on each host) as a
structured session, not a terminal UI. herdsman uses berth's CLI for the two steps berth itself owns: `berth run
--acp <id> --policy <acp_policy> --name … --label … --cwd …` builds the agent's bridge, and `berth send` frames each
prompt. The kick waits for `ACP session ready`, sends the brief, and waits for `working`, `blocked` or `done`. No
Enter is pressed after a prompt. Everything else (read, wait, kill, list) is the same socket call as above.

Read before using it: the ACP agent does not run the harness's own hooks, so **herdsman's tool guard does not apply
in ACP mode**. With `acp_policy = "ask"` (the default) a tool call leaves the lane `blocked` until someone runs
`berth approve`; `allow` lets the agent run every tool, with only GitHub branch protection (`herdsman protection`)
left to stop a push to a protected branch. See berth's `docs/acp.md` for how each agent logs in.

## Setup

1. Install berth on every host that runs lanes, and start its server (`berth serve --detach`, or berth's launchd or
   systemd unit). `berth connect --autostart` starts it on another host when needed.
2. Set `[runtime] kind = "berth"` in fleet.toml on the main host (and on another host's fleet.toml if its lane hooks
   should use a non-default socket; without one, the hook uses berth's default socket).
3. Run `herdsman profiles sync` so the Claude profiles get the state hooks.

## Live trial

The acceptance trial is one herdsman lane on berth end to end (spawn, push, review, merge), run on a real host. Its result goes here.

- Status: not run yet.
