# Scene device listener (#1497)

`[serve] device_addr` (e.g. `"0.0.0.0:7779"`; unset or `""`, the default, is off; a `--read-only` serve has none)
opens a second listener for a LAN device such as the scene-esp32 companion, which cannot join the tailnet. It answers
`GET /screens/scene` only, with the JSON the main listener gives an unpaired device; any other path is a 404 and any
other method (a `POST /action` included) a 405. It reads no cookies, pairs nothing, runs no action and has no
`/stream`: the device polls. Against DNS rebinding its `Host` header must be an IP literal (with an optional port)
or a name in `[serve] device_hosts`, else 403; it sends no CORS headers. `drover doctor` shows `scene device — off`
or `on <addr>`.

Anyone on the LAN can read what the scene shows: each fleet's lane names and states and the host's load, memory
pressure and swap. Leave it off on a network you do not trust.
