# Static export and the public demo

`drover serve --export <dir>` writes the page as static files instead of serving it: `index.html`, `sw.js`,
`manifest.webmanifest`, the icons, the `a/<hash8>/` assets and themes, and each screen as JSON at
`screens/<x>/index.html` (each place's screen, the cards and every sheet they open, such as a lane's), rendered by the
same code as the live page. Every URL in them is relative and the service worker keys its paths and cache on its
registration scope, so the files work under a sub-path (the live server at `/` behaves as before). A plain export is
the read-only view, without transcripts.

#983: `/api/v1/state` has `counts`: `busy` (lanes `working` or `running`), `waiting_approval`, `waiting_owner` and
`rate_limited`; each lane carries shed's `note` on its state (a `rate-limited` lane's reset time).

`--demo` (#39) is the public demo at <https://jirathip-dev.github.io/herdsman/>, built from the synthetic
`demo/` (3 made-up repos, `fleet.toml`, `events.jsonl`, `cache/` for issue/PR titles and CI, and
`runner.json`: the canned agent list, pane text and host values by command-line prefix). Nothing
runs: no shed, no gh, no network, and `demo/` is never written (#344: the export imports its files into a scratch database). "Now" is a minute after the log's last event, so ages never go stale.
The page carries a `demo` badge and a one-line note; an action only says "Demo with fake data", no POST is ever sent,
there is no stream (the dot is a static blue), no pairing and no "enable alerts". Lane sheets are the exported
`screens/lanes/<agent>`. `.github/workflows/pages.yml` publishes it on every push to `staging`; CI exports it too and
runs the leak gate over `docs/`, `demo/` and the export.
