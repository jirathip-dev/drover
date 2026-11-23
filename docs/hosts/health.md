# Health (#235)

`drover doctor` checks every enabled remote host, and `check` does the same once an hour (with the tag probe). A
failure marks the host red (`unhealthy · <reason>`) and keeps new lanes off it: placement refuses it
with `NO_HOST: <host>: unhealthy: <reason>`. The checks, each with its fix:

- **git**: `ssh <host> git ls-remote https://github.com/<repo>.git` works for each fleet whose needs the host fits.
  drover's own git over SSH never has the keychain, so HTTPS fails with `could not read Username for
  'https://github.com'`. A failure reads `✗ git <fleet> — can't reach <repo>: <first error line>` and keeps only
  that fleet's lanes off the host; placement moves them to the next host. Fix: add the host's SSH key to GitHub
  and run `git config --global url.git@github.com:.insteadOf https://github.com/` there.
- **gh** and **claude**: `gh auth status` and `claude auth status` succeed in a login shell on the host
  (`sh -lc`). Lanes there need `gh` for their PRs, so `gh` logged out is
  a red host problem. Fix: `ssh <host> gh auth login` for gh; for claude, log in from a terminal there.
- **drover**: `drover --version` there prints the same version and git sha as on the main host. A mismatch is
  a warning (amber), not a failure.
- **shed**: its lanes list answers there; one that does not answer (#1127: e.g. frozen by a snapshot) is
  `✗ shed — does not answer: … → ssh <host> '<thaw and restart>'`, so it takes no lanes.
