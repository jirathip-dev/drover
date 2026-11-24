# Faults and the nightly soak (#1648)

**Fault harness.** `crates/cli/tests/faults.rs` runs spawn, prompts, remote verdict pickup and the outbox drain over a
fake link that injects a hung shed server, latency, dropped connections, a vanished host, and over ssh an address
change, a host-key change, a vanished host and a hung call. Each test has an explicit time bound. It runs in the
required `test` check, so every change touching multi-host paths must pass it. A call to this host's shed server
with no answer for `[limits] shed_call_secs` (30 s, plus a `wait`'s own timeout) fails instead of blocking the pass.

**Nightly soak.** With `[fleet.<n>.soak]` set, `tick` starts `drover soak <fleet>` once a night, detached:

```toml
[fleet.drover.soak]
hosts = ["mini", "air", "boat-dev-1"]   # [hosts] names
spawn_secs = 30                         # shed's run of the throwaway lane answered within
verdict_minutes = 2                     # its verdict file read back over the host's link within
stuck = 0                               # throwaway lanes that did not come up, write or end
```

On each host it starts a throwaway lane `<fleet>-soak-<host>` that writes a verdict file in `/tmp/drover-soak`, times
the start and the read-back, then kills and removes the lane. The numbers go to the event log as `soak`. Each
missed target files one issue a night (`soak: <fleet> <target> target missed <date>`). `drover soak <fleet>` runs it
by hand and exits 1 on a miss.
