# ACP hosts: `runtime = "shed-acp"` (#637)

```toml
[hosts.boat1]
machine = "boat1"
runtime = "shed-acp"   # unset: the fleet's runtime, as before

[proxy]                # all optional
# port = 41990         # the token proxy, here and (through a tunnel) on each shed-acp host
# upstream = "https://api.anthropic.com"
# key_hours = 12       # how long a lane key lives
```

A `shed-acp` host runs every lane as an ACP session in its own shed (`shed run --acp claude-acp … -- <adapter>`
over ssh). The brief is the first prompt, and every later prompt goes through the same remote shed.

- **Survey.** The host is up when its shed answers (`ls` through `shed connect`); a host whose shed is down is
  offline. Load, memory and disk come from the
  same ssh look as on any host.
- **Health.** git for each fleet it fits, `gh auth status` over ssh, and for each Claude
  role's profile the ACP guard below. A failed guard is a `✗ acp guard <profile>` row, which keeps new lanes off.
- **The guard, proven there (hard gate, as #565).** Before a lane starts, spawn syncs the lane's profile there
  (`drover profiles sync --profile …`) and checks three things: that drover there is this build's version (#573),
  that the profile's `settings.json` has drover's `PreToolUse` hook, and that `shed agents` there lists
  `claude-acp`. If any check fails, spawn refuses `ACP_UNGUARDED <host>: <why>` before anything is made, and records
  the `✗ acp guard` row so the next placement picks another host.
- **No token there: the token proxy.** The lane's adapter gets `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>` and a
  per-lane key as `ANTHROPIC_AUTH_TOKEN`. The key reaches it only through the 0600 secrets file over ssh stdin, which
  is removed once the shell has read it. Port `<port>` there is an ssh reverse tunnel (`ssh -N -R`, started detached
  by spawn and each survey, its pid in `<state>/tunnel-<host>.pid`) to `drover proxy` here, which spawn starts when
  it does not answer. #1335: the tunnel is up only while its ssh lives and a `curl` to `127.0.0.1:<port>` run there
  answers; else the listener a dead session still holds on that port there is killed (`ss -p`, then `sudo -n`) and
  the tunnel restarted, and the survey keeps a `✗ tunnel down` row: no new lane goes there (a respawn leaves it)
  until a look answers again. A lane that dies on `ECONNREFUSED` is gone with cause `ECONNREFUSED: proxy tunnel
  down`, not `adapter crash`. The proxy checks the key, then forwards the call to `upstream` with the one
  `[proxy] token-file` token (#923: `claude setup-token`'s, 0600, [accounts](../accounts.md); only the proxy reads it;
  a remote ACP spawn without it is refused `NO_PROXY_TOKEN` with that step; #1131: so is starting a `shed-acp`
  cloud host, by `hosts bootstrap` or drive, before the recipe's first call, so no VM is made or billed; doctor's
  proxy row, the Hosts screen's Token proxy card and the `proxy-token-expiring` / `proxy-token-rejected` alerts are
  in [accounts](../accounts.md)) and the `oauth-2025-04-20` beta added. curl sends it, with the token in its config on stdin and never in
  argv, and the answer streams back as it comes. The token never leaves the main host.
- **Lane keys.** They are kept in `<state>/proxy-keys.json` as sha256 only. A key signs while it is `live`: it is not
  `revoked`, not `expired` (`key_hours`), and not `ended` (its issue merged or closed, its lane closed, dead or
  spawned again, the last of which revokes the older key at once). `drover hosts keys` lists them with their state.
  `drover hosts keys revoke <id>` (owner only) ends one at once and logs `proxy-key-revoked`. A refused key gets a
  401 and nothing goes upstream.
- **Bootstrap.** `hosts bootstrap <name>` on a `shed-acp` host: git, gh, Claude Code,
  rust, shed once from its latest main (`cargo install --git …/shed --branch main`, skipped when `shed --version` answers;
  shed updates itself after that),
  shed's job (`shed install`, then systemd `enable --now` or launchd `bootstrap`; `shed doctor` is the read-back),
  node and npx, `shed agents add claude-acp`, and drover. No Claude login is opened there. gh is the only login,
  and it must be done by hand over ssh until the git relay lands. `hosts snapshot` saves that state as before.

**Proof (2026-11-08, the mini).** A throwaway proxy on 127.0.0.1 swapped a lane key for the subscription token
(`CLAUDE_CODE_OAUTH_TOKEN`) and added `anthropic-beta: oauth-2025-04-20`. A Claude Code 2.1.311 client was started
with no credentials of its own: `env -i`, a fresh `HOME` and `CLAUDE_CONFIG_DIR`, only `ANTHROPIC_BASE_URL` and
`ANTHROPIC_AUTH_TOKEN=<lane key>`. Run as `claude -p` it answered `PROXY-OK` (exit 0, upstream 200). Run as a shed
ACP session (`shed run --acp claude-acp`, claude-agent-acp 0.101.0, its own shed server, no token in its env) it
answered `ACP-PROXY-OK` (upstream 200). With a wrong key the proxy answered 401 every time, so nothing went
upstream, and the client retried until killed. No `sk-ant` string was in either config folder afterwards.

**Not yet (follow-ups).** The git relay (the lane's origin through the Mini, pushes only to its own branch, PRs from
the Mini, no gh on any remote host). shed's machine registry (`shed machine add`, `--on`), one event stream per
machine, the host grace window, the doctor's cooee warning, and shed-acp as the default runtime. Bootstrap from
sha256-checked prebuilt binaries (needs release artifacts). The `where` field (#609) is not in this repo yet.
