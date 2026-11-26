# The 4 checks in gatekeeper: how the callers read (#1643)

Sketches of the code and commands after the last slice. Names of functions inside a module are the impl lane's;
the calls across modules are the design. Revised 2026-11-26: no cap anywhere (#1812), and the module's calls are
written as D3 merged them.

## A lane, on any host

Nothing changes for the lane, and nothing of drover's is needed on the host: the wrapper, `curl` and git.

```
$ ./.gatekeeper/run check
gatekeeper 1.5.0 · legs core rust · base 7c0a28c1c1a2 · 6 files changed · host lane · cache on
ok    core/pin              0.0s /    1s
ok    core/rules            0.4s /    5s
      1 shadow check would fail: no-todo
ok    core/arch             7.9s /   45s
FAIL  core/hygiene          0.2s /    5s
      private-name #2 at docs/setup.md:14
      fix: take the name out where the hit points; CI passes the private-names secret, …
      log: target/gatekeeper/core-hygiene.log   rerun: ./.gatekeeper/run check --gate core/hygiene
ok    core/pr-body          0.0s /    2s
      no PR body: nothing to check
```

In its own pane a lane has no names list, so in a public repo `core/hygiene` prints `list-missing` there instead
of the hit above; the done gate, which hands the names over, is the run that counts (`adr.md`, open question 2).

## `drover turn done` (done.rs)

The head check stays. The PR body check, the fallback to drover's arch verb and the fallback to `cargo fmt` go.

```rust
let at = git(&["rev-parse", "HEAD"]).unwrap_or_default();
if !at.eq_ignore_ascii_case(&head) {
    return refuse(CODE, format!("push first: the worktree is at {at:.7}, the PR head is {head:.7}"));
}
let limit = Duration::from_secs(cfg.limits.gate_check_minutes * 60);
let pr = gatekeeper::Pr { head: &head, body: text };
let Some(checks) = checks(f, &on, wt) else {
    return refuse(CODE, NO_DONE_CHECK.into());
};
for c in checks {
    let o = if c == gatekeeper::CHECK {
        gatekeeper::check(&on, wt, &pr, &f.private_names, limit)
    } else {
        on.run_for("sh", &argv(&["-c", &format!("cd \"$0\" && {c}"), wt]), "", limit)
    };
    match o {
        Ok(o) if o.code == 0 => {}
        Ok(o) => return refuse(CODE, format!("`{c}` {}:\n{}", what(o.code), tail(&o))),
        Err(e) => return refuse(CODE, format!("`{c}` cannot start: {e:#}")),
    }
}
Ok(Some((head, None)))
```

`checks` is the fleet's `done_checks` when set, else the wrapper's check when the repo holds the wrapper, else
`None`. The loop is D3's, as merged. D5 deletes what still stands before and beside it: drover's own PR body check
(`PR_BODY`) and the two fallbacks. `f.private_names` is read on the main host, where `turn done` is handled; the
lane's host is only where the command runs. A private repo turns `core/hygiene` off, so its list is never read.

## What the module sends (gatekeeper.rs)

Two calls on the lane's host. The body is public text and goes to a file. The names go on stdin: in no argument,
no file and no log.

```rust
on.write(&wt.join("target/gatekeeper/pr-body.md"), pr.body)?;
let line = "cd \"$0\" && DROVER_PRIVATE_NAMES=\"$(cat)\" GATEKEEPER_HEAD=\"$1\" \
            ./.gatekeeper/run check --pr-body target/gatekeeper/pr-body.md";
let out = on.run_for("sh", &argv(&["-c", line, wt, pr.head]), &names.join("\n"), limit)?;
```

## The merge check (gate.rs)

```rust
pub(crate) fn premerge_checks(f: &Fleet) -> Vec<Check> {
    f.premerge.clone().map_or_else(Vec::new, named)
}
```

A fleet's own `premerge` commands run as today, and nothing else: no cap command, no `Cap:` line copied into
the merge commit. That is #1812's diff, not a slice of this work; D3's `gatekeeper::cap` goes with it.

## The lesson lane (lesson.rs)

```rust
pub fn record(ctx: &Ctx, on: &On, wt: &Path, (fleet, n): (&str, u64)) -> Result<()> {
    let Some(report) = gatekeeper::rules(on, wt)? else { return Ok(()) };
    let shadow = report.checks.iter().filter(|c| c.state == CheckState::Shadow);
    let ran: Vec<&str> = shadow.clone().map(|c| c.name.as_str()).collect();
    let hits: Vec<&str> = shadow.filter(|c| !c.passed).map(|c| c.name.as_str()).collect();
    if ran.is_empty() {
        return Ok(());
    }
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "lesson-shadow", json!({"ran": ran, "hits": hits})))
}
```

`lesson apply` runs in the lesson lane's worktree: it calls `gatekeeper::rules`, decides by the history as today
(`review`), and rewrites the lines the report numbers: a promoted one from `shadow since <date>` to `active since
<today>`, a retired one deleted. `prose_only` asks only whether an added rules line carries one of the two marks.

## A check in gatekeeper (check/pr_body.rs)

```rust
/// The PR body names the head, has `## For humans`, has no "Generated with" line; no body: nothing to check.
pub fn run(cx: &Cx) -> Result<Vec<String>> {
    let Some(file) = cx.pr_body else { return Ok(Vec::new()) };
    let text = fs::read_to_string(file).map_err(|e| Error::at(file, e))?;
    let head = match cx.head {
        Some(h) => h.to_owned(),
        None => runner::ok("git", &["rev-parse", "HEAD"], cx.root)?.trim().to_owned(),
    };
    let walk = Walk { root: cx.root, integration: cx.integration };
    Ok(body::faults(&text, &head, &walk).into_iter().map(|f| format!("PR_BODY: {f}")).collect())
}
```

`hygiene` reads `cx.names`; with `None` it returns the one `list-missing` line. `arch` reads `cx.base` and
`cx.pr_body` as the verb read `--base` and `--pr-body`.

## The shared names set, in drover (tests/hygiene.rs)

The guard's side of the set (D2). Gatekeeper's side needs no code: the set is a fixture case, and selftest runs it.

```rust
#[test]
fn the_guard_matches_private_names_as_the_shared_set_says() {
    let set = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/private-names");
    let read = |f: &str| fs::read_to_string(set.join(f)).unwrap();
    let check = Check { private: read("names").lines().map(String::from).collect(), ..Check::default() };
    let hits: Vec<String> = (read("lines.md").lines().zip(1..))
        .filter_map(|(l, n)| Some(format!("{} at lines.md:{n}", check.find(l)?)))
        .collect();
    assert_eq!(hits.join("\n"), read("expect").trim());
}
```

## A repo's one PR

The repo's cap-removal issue, which is also its bump (`adr.md`, decision 13):

```
$ ./.gatekeeper/run pin 1.5.0
$ git rm -r ci/cap.toml ci/cap.d
```

and in its gatekeeper config, while the pinned release still has the gate:

```toml
[off]
"core/cap" = "the line cap is removed (drover#1812)"
```

A PR that pins a release without `core/cap` (gatekeeper#127) adds no such line; a repo that has one deletes it
at its next bump. A private repo adds:

```toml
[off]
"core/hygiene" = "private repo: nothing here is published"
```

Drover's own config (#1587) has no `[tools]` table, and its `arch.toml` loses `cli = true`.

In drover, a bump also runs `just gen`, which rewrites the shared names set from the new pin. If gatekeeper
changed how a name matches, the test above fails until the guard matches the same way, in that PR.

## The proofs the issue asks for

```
# in gatekeeper, after G5 (run 2026-11-26 at v1.5.0: no line, exit 1)
$ git grep -nE 'drover (cap-check|lesson-check|arch-check|hygiene check|pr-body)' v1.5.0 -- packs ; echo $?
1

# in drover, after D5: the done gate against a lane host with no fleet config (D3's test, still green)
$ cargo test -p drover --test done_gate a_remote_lane_with_no_fleet_config_passes_turn_done
```
