# Brief: write or refresh this repo's verify recipe (#417)

An impl lane's task. herdsman's verify stage drives the real app at a PR's head by following
`.herdsman/verify.md`; this issue writes that file (or brings it up to date) and proves it.

## Interview the repo, not the owner

Read the README, the justfile / package scripts / Makefile, CI workflows, the dev-server and test
setup, and the UI's routes or screens. Every answer comes from the repo or from running it. Ask the
owner only for what no file and no run can tell you (a test login, say), with `herdsman ask`.

## The file: `.herdsman/verify.md`, harness-neutral, six sections

- **Launch:** the exact command that starts the app at the checked-out head, its ready signal
  (a log line, a URL that answers), and its teardown. Use `$HERDSMAN_PORT`, never a fixed port;
  iOS through `sim-task` (the host's `hermes-sim-task` is the same tool). No containers:
  `deny_commands` refuses them.
- **Doctor:** one read-only check that this instance is worth driving (it serves, it has data).
- **Drive:** the real selectors, URLs and commands this repo's UI answers to.
- **Evidence:** how to take screenshots at 1440 and 390 px wide, where the transcript goes, which
  exit codes to record. Files go in the lane's own folder. A web UI: each screen's
  `node .verify-browser.mjs <url> shots --name <screen> --recipe .herdsman/verify.md`
  (a headless or headful Chrome DevTools session; `CHROME` names the browser): the screenshots, its
  console and network logs, an a11y report, `--trace` for a speed criterion, `--approved <dir>` and
  `--threshold <%>` for the design diff. iOS: `sim-task` screenshots, diffed with its `diff` mode.
- **Known noise:** one bullet per console or network line that is no bug (a substring of it, say
  a third-party warning); any other console error or failed request fails verify.
- **Feature map:** each screen or feature → how to reach it.

## Prove each step

Run every command in the recipe from a clean checkout and paste the output in the PR body: the
launch reaching its ready signal, Doctor passing, one Drive path per screen, both screenshots,
the teardown. No placeholders, no `TODO`, no step you did not run.
