#!/usr/bin/env bash
# #928: pin the theme packs to commits of the shared repo. `scripts/swag.sh <commit> <bush commit>` replaces assets/swag/
# with the first commit's legacy-board/{farm,kitchen,garden} and (#1418) bush built by scripts/swag-bush.mjs from the
# second's design/bush, and writes SOURCE and SHA256SUMS; the themes test checks them.
set -euo pipefail
sha=${1:?usage: scripts/swag.sh <40-hex commit of the swag repo> <40-hex bush commit>}
bush=${2:?usage: scripts/swag.sh <40-hex commit of the swag repo> <40-hex bush commit>}
for s in "$sha" "$bush"; do [[ $s =~ ^[0-9a-f]{40}$ ]] || { echo "not a full commit sha: $s" >&2; exit 2; }; done
repo=jirathip-dev/swag
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/legacy" "$tmp/bush"
curl -fsSL "https://codeload.github.com/$repo/tar.gz/$sha" | tar -xz -C "$tmp/legacy" --strip-components=2 "swag-$sha/legacy-board"
curl -fsSL "https://codeload.github.com/$repo/tar.gz/$bush" | tar -xz -C "$tmp/bush" --strip-components=1 "swag-$bush/design/bush"
rm -rf assets/swag
mkdir -p assets/swag
for t in farm kitchen garden; do cp -R "$tmp/legacy/$t" assets/swag/; done
node scripts/swag-bush.mjs "$tmp/bush" assets/swag/bush
printf '%s\n' "$repo legacy-board at $sha" "$repo design/bush at $bush" > assets/swag/SOURCE
(cd assets/swag && find farm kitchen garden bush -type f | LC_ALL=C sort | xargs shasum -a 256 > SHA256SUMS)
echo "assets/swag pinned to $repo@$sha, bush@$bush"
