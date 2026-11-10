Closes #786

## For humans
The owner can now see every fleet setting from the phone app, with each setting's allowed values and a one-line help note. Secrets appear only as their names.
They can check a change before saving it, and save it either field by field or by editing the whole file. If the file changed after they opened it, the save is refused.
They can add a repo as a new fleet, remove a fleet's settings (a fleet with lanes still running is refused unless confirmed), and pause or resume a fleet. Comments in the config file are kept, a backup is saved, and every change is logged.

## What changed
- `GET /api/v1/config` (paired device only) returns the file's sha256 `hash`, the raw `text`, and `fields`. Each field has: dotted `path`, TOML `kind`, `value`, `allowed` (harness, effort, and model from `models`), `doc` (the comment on that key in fleet.example.toml) and `secret` (the NAME of a `secret:NAME`). A file holding a secret-shaped value is refused with `SECRET_IN_CONFIG`, and the value is not echoed back.
- `POST /api/v1/config-check` takes a `ConfigChange` (raw `text`, field `set` edits, `add` fleet, `remove` fleet) and returns `{diff, errors}`. Nothing is written.
- `POST /api/v1/config-edit` applies the owner's edit:
  - refused `STALE` if the file no longer matches `hash`;
  - refused `BAD_VALUE` if the change is invalid or changes nothing, and nothing is written;
  - refused `LANE_LIVE` when removing a fleet that has live lanes, unless `confirm` is set.
  
  On success it takes the same path as `config apply`, now shared in `propose::install`: backup, profiles sync, then doctor. It logs a `config-edit` event with `by: owner (cooee)`.
- Adding a fleet appends `[fleet.<name>]` (repo, local, integration) and copies the first fleet's `[fleet.<first>.role.*]` sections. Removing a fleet drops only its `[fleet.<name>]` and `[fleet.<name>.*]` sections.
- Pause and resume already exist as `/api/v1/pause` and `/api/v1/resume` (logged `pause`/`resume`, `via: web`). They are unchanged; the new test covers their round trip.
- Field edits go through `config::set_in`, which keeps comments and only rewrites the value bytes. It previously assumed scalar values; it now handles any value up to an unquoted `#`, which covers arrays and type changes. No `toml_edit` dependency is added.
- TS types: `ConfigView`, `ConfigField`, `ConfigChange`, `ConfigSet`, `NewFleet` and `ConfigCheck` are added to `assets/types/api.d.ts`.

Decisions:
- A new fleet's roles are "the defaults" = the first fleet's roles.
- `errors` lists the first parse error only, because `Config::parse` stops at the first; collecting all of them would mean rewriting its validation.
- No new dependency.

src/ net +256 (no budget given). The read covers every setting plus a typed contract for cooee. The cap check passes: 38023 / 38028.

## Evidence (head 33cb8f6cea8cddc35ea3936204e07e54e1acd890)
- `cargo test -p herdsman --test config_api` → exit 0 (4 passed). This is the new test file; it drives the real `/api/v1` routes.
- `cargo test -p herdsman --test cloud --test chat_tab --test config_api --no-fail-fast` → exit 101. The one failure is `chat_tab::the_thread_follows_its_end_until_you_drag_and_drafts_survive_a_reload`, which fails the same way on staging 3004bbd2 (checked in a scratch checkout). It is a pre-existing headless-browser layout failure.
- `cargo test -p herdsman --test config_api --test config_card --test api_v1 --test api_v1_ops --test api_v1_contract --no-fail-fast`: api_v1 8/8, api_v1_contract 2/2, api_v1_ops 7/7 and config_api 4/4 ok. In config_card, the non-browser tests that cover the shared apply step passed, including the new-fleet kick and role profile sync. Its WebKit test `the_card_lands_live_in_view_and_stays_reachable_in_webkit` was still hanging after 60 s+ when the lane ended.
- `cargo clippy --all-targets --features herdsman-server/ts -- -D warnings` → exit 0
- `cargo test -p herdsman-server --features ts --test types` → exit 0 (types regenerated)
- `herdsman cap-check --config ci/cap.toml` → 38023 / 38028
- I never got a complete run of the full workspace suite: twice the lane's session ended before it finished. CI runs it.

## Why (round 1)

Decided: structured edits go through the existing line editor config::set_in (now any value type) instead of a new toml_edit dependency.
Because: "Comments in fleet.toml are kept through structured edits (a TOML-preserving editor)."
Rejected: toml_edit as a direct dependency — a new dependency needs an owner ask, and set_in already edits value bytes only.
Decided: a manual edit shares proposal apply's install step (backup, profiles sync, doctor) and logs config-edit by "owner (cooee)".
Because: "Backup kept, `doctor` run, event logged `by: owner (cooee)`, as `config apply`."
Decided: pause/resume reuse the existing /api/v1/pause|resume board ops; the new test only covers their round-trip and log.
Because: crates/server/src/serve.rs:1428
Decided: a new fleet copies the first fleet's role sections; remove drops only [fleet.X] and [fleet.X.*] sections.
Because: "remove a fleet (only its config section; never its repo, worktrees or log; refused while it has live lanes unless confirmed)"
Rejected: reporting every parse error at once — Config::parse stops at the first; collecting all would rewrite its validation.
Check: crates/cli/tests/config_api.rs drives the real /api/v1 routes: secrets as names, STALE, invalid edit writes nothing, comments kept, add/remove/pause/resume logged.
Owner: cooee can now read, check and directly edit fleet.toml, add or remove a fleet, and pause or resume one, with comments kept.

<sub>note on 33cb8f6 · git log --notes=why</sub>

