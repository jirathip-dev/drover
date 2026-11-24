# Turn a host off

`drover hosts off <name>` sets `enabled = false` in fleet.toml and logs `host-off` with `by`. `hosts on` undoes it,
logging `host-on`.

An off host gets no new issues, and its running lanes drain there: their reviews and fix rounds still go to it. `GET /api/v1/host` shows such a host grey, as `off · draining 2 lanes` while lanes remain,
then `off · not used for lanes`. The main host (no `machine`) runs drover, the page and the log: `hosts off` refuses it `MAIN_HOST`; set `enabled = false` in fleet.toml by hand if you mean it.

#1357: `kick`, `drive`, `check`, `waiting`, `status` and the tick never contact an off host (no ssh at all), lanes
there or not; only a hosts refresh reads it, its lanes' verdicts included. A host one ssh connect fails is offline
for the rest of that command and, through `<state dir>/hosts.down`, for every other drover command for `host_down_secs`
(60 s; the tick probes afresh and renews it): one dead host costs a pass one connect timeout, not one per lane. `doctor` shows it as one line,
`unreachable (ssh: …), down 12m`. A kick running past `kick_max_secs` (300 s) is ended by the next tick's `check` and logged
`kick-killed`; a second kick of an issue while one runs exits at once, logged `kick-in-flight`, and the running one
drives it after. Every remote call in a pass is killed with its
ssh's process group after `ssh_call_secs` (20 s), output still held open included; #1544: a second such call takes the host offline for the
rest of that pass (a host listing killed once is sent once more) and the other hosts and fleets go on. A start held as
`HOST_OFFLINE` first reaches each offline host once more directly (ssh and its shed listing) and, if one answers, starts in
the same pass. An ssh refused with `REMOTE HOST IDENTIFICATION HAS CHANGED`
logs `host-key-changed <host>` once (again only after the host was back online) and is not retried in that pass.
#1469: at most `ssh_sessions` (8) remote calls run at once over one host's shared connection, the rest wait their turn,
under sshd's `MaxSessions`; a call the shared connection refuses (`Session open refused by peer`) runs again on it
(`ssh_retry_secs`, 3 times) and never marks the host offline. `hosts bootstrap` writes
`/etc/ssh/sshd_config.d/10-drover.conf` (`ClientAliveInterval 60`, `ClientAliveCountMax 3`, `MaxSessions` 4 ×
`ssh_sessions`), so a dead client's session ends within 3 minutes; `doctor` shows one `host <name> sshd` row per remote
host, `!` when keepalive is off, `MaxSessions` is under 2 × `ssh_sessions` or over 20 `notty` sessions have no child.
