#!/usr/bin/env bash
# #928: pin the theme packs to a commit of the shared repo. `scripts/swag.sh <commit>` replaces assets/swag/ with that
# commit's legacy-board/{farm,kitchen,garden} and writes SOURCE and SHA256SUMS; the themes test checks them.
set -euo pipefail
sha=${1:?usage: scripts/swag.sh <40-hex commit of the swag repo>}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo "not a full commit sha: $sha" >&2; exit 2; }
repo=jirathip-dev/swag
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://codeload.github.com/$repo/tar.gz/$sha" | tar -xz -C "$tmp" --strip-components=2 "swag-$sha/legacy-board"
rm -rf assets/swag
mkdir -p assets/swag
for t in farm kitchen garden; do cp -R "$tmp/$t" assets/swag/; done
echo "$repo legacy-board at $sha" > assets/swag/SOURCE
(cd assets/swag && find farm kitchen garden -type f | LC_ALL=C sort | xargs shasum -a 256 > SHA256SUMS)
echo "assets/swag pinned to $repo@$sha"
