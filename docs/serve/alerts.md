# Alerts (web push)

`[⚙]`'s **enable alerts on this phone** shows only in the Home Screen app, to a paired device (iOS allows web push
there alone). Tapping it asks for notification permission, subscribes with the server's VAPID
public key and `POST /push/subscribe`s the subscription; that and `POST /push/unsubscribe` need the same Host/Origin check and a paired device (401
otherwise). Delivery goes Mac → Apple/Google push service → phone, outbound only; the page stays
tailnet-only.

#370: the subscription carries the phone's theme words (`w_merged`, `w_needs`), so a merge reads `Order up: #2957
merged` (kitchen) or `Over the fence: #2957 merged` (farm), and needs-human / an owner ask `The bell: api #2961 needs
you` / `At the gate: …`; terminal sends none and keeps today's titles. A theme switch re-sends them with `quiet=1`
(saved, no test push). Only the title changes; the body stays.

```toml
[[notify]]
type = "webpush"
sub = "mailto:you@example.com"   # the VAPID `sub` claim; required
# on = [...]   # default: every alert emitted as a push (blocked, stall, lane-dead, merged, needs-human, ask-waiting, …)
```

#1147: with no `on`, a phone gets each event logged with `"push": true` (set where the alert is born,
`Event::push`) and the kinds pushed before the flag (`crates/core/src/events/push.rs`, frozen: it only shrinks).

It works per fleet too (`[[fleet.<name>.notify]]`), next to any command notifier, which is
unchanged. Each event is encrypted per device (RFC 8291 aes128gcm), signed with an ES256 VAPID
JWT (12 h) and sent once with `curl -sS --max-time 10 -X POST`, the body on stdin. A 404 or 410
prunes that device; any other failure logs one `notify-failed` (no retry). Tapping an alert
opens the page on its tab with the issue's row open. `drover push test` sends a test alert to
every subscribed device.

The subscribe tells the truth (#87): with a webpush notifier it pushes one "alerts are on" test to
that device alone and answers `alerts on` (or why the test push failed); without one it still keeps
the subscription but answers, under the button, "alerts are saved but off: add `[[notify]] type =
"webpush"` to fleet.toml (docs/serve.md)". API v1's `host` `push` row reads `on · 1 device · last
sent 3m ago`, `off: no webpush notifier`, or `on · last failure: <reason>` while a webpush
`notify-failed` is newer than the last delivery. Each delivery logs one passive `push-sent` with the
device count only (no endpoint, no key). `drover check` logs one `alerts-off` (again only when the
device count changes) while `subs.json` has devices but fleet.toml has no webpush notifier.

Secrets: the VAPID key (`<state>/webpush/vapid.key`) and the subscriptions
(`<state>/webpush/subs.json`, endpoint, keys, device label, creation time) are 0600 and never
logged, printed or served; the page gets only the public key and each device's label.

Dependencies: the allowed crates are clap, serde, serde_json, toml and anyhow/thiserror, plus, for web
push only, the pure-Rust RustCrypto set p256, aes-gcm, hkdf, sha2 and base64 (no async, no HTTP
client), ts-rs behind the `ts` feature for the board's TS types (#375, never in the release build), and, for the
shed runtime only, shed-proto (shed's serde wire types, no I/O; docs/runtimes.md), blocks and swag (#1339: the
theme rules and the built-in colour schemes, each pinned to a commit), and rusqlite (bundled SQLite)
for the state database (#344, docs/architecture.md), keyring for the main host's secret store
(#441, owner-approved; docs/providers.md), flate2 for serve's gzip (#1314, owner-approved), ratatui (with its crossterm backend) in `crates/chat` only (#1352, owner-approved, q2151), and tokio in `crates/service` (#345, docs/service.md) and in `crates/server` for serve's connections (#1318). gatekeeper's core/deps
fails on any direct dependency not listed with its reason in `.gatekeeper/deps.toml`.
