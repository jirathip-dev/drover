# GitHub webhooks (#626)

serve also listens on `[serve] hooks_addr` (default `127.0.0.1:7778`, `""` turns it off; a `--read-only` serve has
none). That listener answers `POST /hooks/github` only: every other path is a bare 404, so publishing it never
publishes the board. It is the one path on Tailscale Funnel; the board stays tailnet-only on `tailscale serve`. Funnel
opens a whole HTTPS port, so give it one the board does not use (443, 8443 or 10000):

```
tailscale funnel --bg --https=10000 --set-path /hooks/github http://127.0.0.1:7778/hooks/github
```

In the repo's GitHub settings add a webhook: payload URL `https://<mac>.<tailnet>.ts.net:10000/hooks/github`,
content type `application/json`, the secret, and the events *Pull requests*, *Check suites*, *Check runs*,
*Statuses* and *Pushes*. The secret is the 0600 file `[serve] webhook_secret_file` names (#707; one for every fleet, refused
`SECRET_FILE_OPEN` if others can read it); without it the endpoint answers 503 and doctor shows
`! webhook — no secret`.

Each delivery's `X-Hub-Signature-256` (HMAC-SHA256 of the raw body) is checked in constant time; a missing or bad one
gets 401 and is only counted (doctor's `webhook` row). A body over GitHub's 25 MB is refused (413) before it is read.
`ping` answers 200. A `pull_request`, `check_suite`, `check_run`, `status` or `push` event naming an open fleet
issue's branch or PR (a merge push: its `(#N)`) re-reads that PR into the cache `drover pr` reads and kicks drive
for the issue at once (200); anything else is ignored with 202. While a fleet has the secret, the fast step's PR read
becomes a catch-up poll every `[limits] webhook_poll_minutes` (default 10), for what a sleeping Mac or a down Funnel
missed.
