# Jobs (#1756)

A scheduled job is one config entry. `drover tick` runs each due job of each fleet that is not paused; `drover job
<fleet>` lists them and `drover job <fleet> <name>` runs one now (orchestrators and the owner).

| Key | What |
|---|---|
| `every` | `"12h"`, `"1d"`, `"7d"`; `0` or `"off"`: only by hand |
| `check` | a command run in a checkout of the integration head: it passes, nothing to do; it fails, the job files and its output is `{report}` |
| `title`, `body` | the issue; `{fleet}`, `{integration}`, `{date}`, `{monday}`, `{month}`, and `{report}` in the body |
| `role` | the lane that works the issue; a role the fleet does not configure runs on impl's |
| `queue` | `top` (default: its role spawned at once) or `end` (drive starts it in turn) |
| `empty` | set: a lane that ends with nothing pushed closes its issue with this line and its undo list |

Three layers, merged key by key: the built-in entry in `roles/jobs/`, then `[job.<name>]`, then
`[fleet.<n>.job.<name>]`. A missing `title`, `body` or `role`, a bad `every` or an unknown key is a load error.

Built in, all on: `lesson` every 7d (files when the week holds 2 review FAILs), `triage` 1d, `deps` 30d, `advisory`
1d (files when the repo's audit fails). A fleet opts out with one line:

```toml
[fleet.web.job.triage]
every = "off"
```

At most one issue per job is open, and a title that exists is never filed again. A job that fails to file logs
`job-failed`, which alerts. The old `[fleet.<n>]` keys `lessons`, `triage` and `deps` are still read under the
fleet's own entry, with a doctor warning naming the lines that replace them.
