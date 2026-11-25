# Shapes: the entry, the built-in entries, the events

## The entry

Three layers, merged key by key: the built-in file, then `[job.<name>]`, then `[fleet.<n>.job.<name>]`.

```rust
/// One layer of a job as written: every key optional.
#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct JobToml {
    pub every_days: Option<u64>,
    pub check: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub role: Option<String>,
    pub queue: Option<Place>,
    pub empty: Option<String>,
}

/// A job after its layers are merged and checked: what `jobs::run` trusts.
pub struct Job {
    /// 0: on demand only (`drover job <fleet> <name>`).
    pub every_days: u64,
    /// A command run in a checkout of the integration head. It fails: the job files, its output is `{report}`.
    pub check: Option<String>,
    pub title: String,
    pub body: String,
    /// The lane that works the issue. An architect or design role is the queue mark, not a spawn.
    pub role: String,
    pub queue: Place,
    /// Set: a lane that ends with nothing pushed closes its issue with this line.
    pub empty: Option<String>,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// On top of the queue, its role spawned at once.
    #[default]
    Top,
    /// At the end; drive starts it in turn.
    End,
}

pub enum Trigger {
    Tick,
    /// `drover job <fleet> <name>` or `POST /api/v1/lesson`: who ran it, when a paired device did.
    Hand { by: Option<String> },
}

pub enum Ran {
    /// The check passed, an issue of this job is open, or the title exists: the line says which.
    Nothing(String),
    /// The issue, and whether its lane's prompt was confirmed (false: exit 4, as `spawn`).
    Filed { issue: u64, confirmed: bool },
}
```

Checked when the config loads (`Config::parse`), never later:

- `title`, `body` and `role` are set after the merge. A new job with one missing is a load error naming the job.
- The rendered title has no `{` left.
- `role` is one the fleet configures, a built-in role, or falls back to impl's harness (today's `role_key` rule for
  lesson, triage and deps, now for any role a job names).

## Placeholders

| Placeholder | Value | Where |
|---|---|---|
| `{fleet}` | the fleet's name | title, body, check |
| `{integration}` | its integration branch | title, body, check |
| `{date}` | today, `2026-11-25` | title, body |
| `{monday}` | this week's Monday | title, body |
| `{month}` | `2026-11` | title, body |
| `{report}` | the check's output, its last 200 lines, the checkout's path replaced by `.` | body |
| `{drover}` | the running binary's path | check |

Any other brace text stays as written.

## The five built-in entries

One file each in a `jobs` folder under `roles/`, collected by `crates/core/build.rs` as the flow tables are. The
bodies are today's issue templates (the lesson issue template and its like), moved into the entry.

```toml
# roles/jobs/lesson.toml
every_days = 7
check = "{drover} lesson digest {fleet} --days 7 --min-fails 2"
title = "lessons: {fleet} week of {date}"
body = """…roles/lesson-issue.md, unchanged…"""
role = "lesson"
empty = "no repeated class"
```

```toml
# roles/jobs/triage.toml
every_days = 0
title = "triage: {fleet} week of {date}"
body = """…roles/triage-issue.md, unchanged…"""
role = "triage"
empty = "nothing to triage"
```

```toml
# roles/jobs/deps.toml
every_days = 0
title = "deps: {fleet} {month}"
body = """…roles/deps-issue.md, unchanged…"""
role = "deps"
```

```toml
# roles/jobs/advisory.toml
every_days = 0
check = """
{ [ ! -f Cargo.lock ] || cargo deny check advisories; } &&
{ [ ! -f pnpm-lock.yaml ] || pnpm audit; } &&
{ [ ! -f package-lock.json ] || npm audit; }"""
title = "deps: advisories {date}"
body = """
Security advisories against `{fleet}`'s `{integration}` head, worked by a `deps` lane at once (#1488).

{report}

…steps 1 to 3 of roles/deps-advisory.md, for every advisory above, in one PR…"""
role = "deps"
empty = "no advisory to fix"
```

```toml
# roles/jobs/hotspots.toml
every_days = 1
check = "{drover} hotspots {fleet}"
title = "hotspots: {fleet} week of {monday}"
body = """
{report}

…roles/brief-hotspots-architect.md, unchanged…"""
role = "architect"
queue = "end"
```

Two verbs gain an exit code so they can be a check, as `hygiene check` and `protection` have one:

- `lesson digest` with a new `--min-fails N` flag exits 1 when the window holds N or more FAIL verdicts.
- `drover hotspots <fleet>` exits 1 with any hot file. It stores the list for drive, as today.

## Events

| Event | On | Data | Notified |
|---|---|---|---|
| `job-filed` (new) | the issue | `{"job": "lesson", "role": "lesson", "title": "…", "by": "web:…"}` (`by` only by hand from a device) | no |
| `job-failed` (new) | the fleet | `{"job": "advisory", "why": "gh issue create: …"}` | yes, as an alert |
| `closed` (exists) | the issue | `{"why": "<the lane's line>\n- <what it changed> · undo: `…`"}` | as today |

`job-filed` is what the rest of drover reads:

```rust
/// The job that filed issue `n`, from its `job-filed`.
pub fn of(events: &[Event], fleet: &str, n: u64) -> Option<&str>;
/// The role its lane runs as.
pub fn role(events: &[Event], fleet: &str, n: u64) -> Option<&str>;
/// Its entry's `empty` line, when the lane may end without a PR.
pub fn empty<'a>(cfg: &'a Config, events: &[Event], fleet: &str, n: u64) -> Option<&'a str>;
```

The undo list in `closed` is generic: every event the lane logged on its own issue that carries an `undo` key, as
`events::plain` prints it. Today those are `triage-closed`, `triage-folded`, `triage-labeled` and `triage-docs`.

No event for a skip. A check that passes, an open issue or an existing title is one line in `tick.log` and the
`job <fleet>` list's `last`.
