# fleet.toml: serial groups (#294)

A serial group is a set of paths only `max` issues at a time may change. Each fleet may have any number; each group has
its own limit and its own Queue mark (🔒name).

```toml
[[fleet.app.serial]]
name = "db"                    # ≤ 8 chars, also the Queue mark
paths = ["supabase/migrations/**"]
max = 10                       # default 1

[[fleet.app.serial]]
name = "ci"
paths = [".github/workflows/ci.yml", "scripts/lib/ci*", "scripts/check-*"]
max = 1
```

The old form, `serial_paths` / `serial_max` / `serial_label`, still works: it is one group named `serial_label` (unset:
`db`). Giving both forms in one fleet is a config error; `drover doctor` suggests the new form.

## Joining

An issue is in a group when

- it is **marked**: `drover queue add N --serial ci`, or `queue set N serial ci,db` (replaces its marks). A plain
  `--serial` means the fleet's first group, as before; or
- its **pushed diff** touches one of the group's paths: drive checks each open PR and logs
  `serial-joined {group, by: "diff"}`.

`drover merge` still refuses `SERIAL_UNMARKED ci: PR touches scripts/check-x.mjs` for a PR touching a group it never
joined (belt and braces).

## Slots

An issue holds a slot in a group from joining until it is merged, closed or parked (in flight: an impl/fix lane, at
review or ready).

- A slot gates only new impl work: a marked issue with no open PR starts only with a free slot:
  `SERIAL_BUSY ci: #2968 holds 1/1`. A blocked issue holds no slot.
- A pushed head is never held (#1136): an open PR whose diff joins a full group is reviewed, verified, fixed and merged
  like any other, and counts toward the group's use. A conflict at merge gets an update round as usual.
- Issues already in flight over a limit when a group is added keep going (grandfathered).

`status` and `board` show each group's use: `serial: db 3/10 · ci 1/1`.

Hot files (#891): each file the last stored hotspots list names is its own one-slot group, `hot <file>`, joined
only by a PR's diff (never a `--serial` mark). Drive drops a file from it on its next pass once
`.drover/arch.toml` on the integration branch lists it `frozen`, `generated` or `shared` (#1147; `hot-files {files}`
logs each change).
