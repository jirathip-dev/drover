# Troubleshooting: macOS Keychain and remote hosts

Pitfalls that each look like a different bug. One section each: symptom, cause, fix, confirm. Never paste a password
or token on a command line: read it with `read -rs` into a shell variable and `unset` it afterwards.

## 1. A locked or jammed login Keychain (#707)

**Symptom.** Lanes on a folder account come up signed out, or `security` calls hang over ssh; the board still serves
and drive still starts lanes (herdsman itself reads no Keychain item and holds no secret store since #707).

**Cause.** Claude Code keeps a folder login (`CLAUDE_CONFIG_DIR=<folder> claude /login`) in the login Keychain. A locked
login Keychain, or unanswered Keychain dialogs queued in SecurityAgent with no one at the screen (seen on 2026-11-08),
leaves that login unreadable.

**See it.**

```sh
ps -axo pid,etime,command | grep -E '[S]ecurityAgent|[s]ecurity (find|unlock)'
```

A SecurityAgent with a long elapsed time plus `security` processes that never finish is the jam.

**Fix.** Over ssh, name the login keychain explicitly (an ssh session has no login search list), unlock it, and kill a
jam's waiting `security` calls by pid, then SecurityAgent (macOS restarts it; the queued dialogs count as Deny):

```sh
K=~/Library/Keychains/login.keychain-db
read -rs P   # the login keychain's password, no echo
security unlock-keychain -p "$P" "$K"
unset P
ps -axo pid,command | grep '[s]ecurity find-generic-password'   # note the pids
kill <pid> <pid>
kill "$(pgrep -x SecurityAgent)"
```

A headless host that must not depend on the login Keychain at all: give its accounts a hand-placed `token-file:`
([accounts](accounts.md)).

**Confirm.** `herdsman accounts` shows each account `signed in`, and a new lane on it starts signed in.

## 2. herdsman's old Keychain items

**Symptom.** `herdsman/<fleet>/<NAME>` items (old `secret:` values, `BOAT_API_KEY`, account tokens) are still in the
login Keychain after the move to secret files.

**Cause.** herdsman stopped reading them (#707) and deletes nothing itself.

**Fix.** Put each value you still need in its file (`<config dir>/secrets/<NAME>`, mode 0600; see
[providers](providers.md)), then run the `security delete-generic-password -a herdsman -s …` lines `herdsman migrate`
prints, naming the login keychain over ssh as in §1.

**Confirm.** `security dump-keychain ~/Library/Keychains/login.keychain-db | grep '"herdsman/'` prints nothing.

## 3. Stale `~/.local/bin/herdsman` on a remote host

**Symptom.** A remote host was updated (`host-update` logged, `herdsman --version` there is current), yet its lanes'
hook behaves like an old build.

**Cause.** The updater installs into `~/.local/share/herdsman/bin`, but the lanes' hook called a separate, older copy
in `~/.local/bin` that is not a symlink, so the update never reached it. #573 is the real fix.

**Check.**

```sh
ssh <host> 'ls -l ~/.local/bin/herdsman; ~/.local/bin/herdsman --version'
```

A regular file (no `->`) or a version different from the main host's is the stale copy.

**Fix.** Replace it with a symlink to the updated binary:

```sh
ssh <host> 'ln -sf ~/.local/share/herdsman/bin/herdsman ~/.local/bin/herdsman'
```

**Confirm.** The check above shows `~/.local/bin/herdsman -> …/.local/share/herdsman/bin/herdsman` and the main
host's version and git sha.

## 4. Remote fleet.toml drift

**Symptom.** Every tool call of a lane on a remote host is refused `REFUSED HOOK_FAILED_CLOSED: <fleet> not in
fleet.toml`.

**Cause.** A remote host's own `~/.config/herdsman/fleet.toml` is written once at cloud bootstrap (only when it has
none) and never refreshed. A fleet added on the main host later is missing there, so the host's lane hook fails
closed.

**Workaround until #592 lands.** Copy the main host's `[fleet.*]` tables (without notifiers) into the remote file, or
move it aside and bootstrap again so it is rewritten:

```sh
ssh <host> 'mv ~/.config/herdsman/fleet.toml ~/.config/herdsman/fleet.toml.old'
herdsman hosts bootstrap <host>
```

**After #592.** herdsman keeps the remote fleet.toml in step with the main host's, so a new fleet reaches every host
without this step.

**Confirm.** `ssh <host> 'grep -n "^\[fleet.<fleet>\]" ~/.config/herdsman/fleet.toml'` prints the table, and the lane's
next tool call is not refused.

## 5. Shed on remote hosts

**Symptom.** With a shed runtime configured, a remote host shows offline and takes no lanes, though ssh and herdr work
there.

**Cause.** Until #601 lands, a shed runtime makes herdsman look for shed on every host; a remote host with no shed fails
that look and counts as offline.

**How to tell.**

```sh
herdsman hosts                    # the host: offline
ssh <host> herdr agent list       # works
```

Offline in `herdsman hosts` while `herdr agent list` over ssh is OK is this case, not a network fault.

**Fix.** Until #601, run remote lanes with the herdr runtime (`kind = "herdr"`, see [runtimes](runtimes.md)), or
install shed on that host.

**Confirm.** `herdsman hosts` shows the host reachable, and a lane placed there starts.
