# Tags

Each `drover check` (and `drover hosts refresh`) looks at every host. Once an hour it probes the host's tags with
one shell line: `os:macos` or `os:linux`, `arch:arm64` or `arch:x86_64`, `xcode` (when `xcodebuild -version` works) and
`docker` (when `docker info` works) and `webkit` (when the Playwright cache under `~/.cache/ms-playwright` or
`~/Library/Caches/ms-playwright` holds a `webkit-*` browser). The results go to `<state dir>/hosts.json`, along with the host's `$HOME`,
whether it answered, and its load/cpu, free memory and free disk. A host's tags are the detected ones plus its hand
`tags`.

## Needs

A lane needs the union of three lists:

- the fleet's `needs` (`[fleet.<n>] needs = ["xcode"]`);
- the role's `needs` (`[fleet.<n>.role.design] needs = ["os:macos"]`);
- the queue item's needs (`drover queue <fleet> add 12 --needs docker,gpu` adds to them;
  `drover queue <fleet> set 12 needs docker` replaces them; `queue ls` shows them).
