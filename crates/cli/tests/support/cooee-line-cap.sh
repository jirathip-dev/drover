#!/bin/sh
# Non-test lines across crates/*/src, web/src and web/ts must stay at most line_cap in ci/cap.toml.
# A Rust line is non-test when it is not blank and comes before the file's first `#[cfg(test)]`
# (unit tests live at the end of a file); every non-blank line of web/src's other files and of web/ts/*.ts counts.
# Non-blank lines of apple/**/*.swift must stay at most apple_cap, leaving out test targets (*Tests/),
# `#Preview { … }` blocks and preview or design fixtures (apple/design/, *Preview*.swift, Fixtures/).
set -eu
cd "$(dirname "$0")/.."
cap=$(sed -n 's/^line_cap *= *\([0-9]*\).*/\1/p' ci/cap.toml)
apple_cap=$(sed -n 's/^apple_cap *= *\([0-9]*\).*/\1/p' ci/cap.toml)
n=$( {
  find crates/*/src web/src -name '*.rs' -exec awk '/^[[:space:]]*#\[cfg\(test\)\]/ { exit } NF { n++ } END { print n + 0 }' {} \;
  find web/src -type f ! -name '*.rs' -exec awk 'NF { n++ } END { print n + 0 }' {} \;
  find web/ts -name '*.ts' -exec awk 'NF { n++ } END { print n + 0 }' {} \;
} | awk '{ s += $1 } END { print s + 0 }')
m=0
if [ -d apple ]; then
  m=$(find apple -name '*.swift' ! -path 'apple/design/*' ! -path '*Tests/*' ! -path '*/Fixtures/*' \
    ! -name '*Preview*.swift' -exec awk '
      !skip && /^[[:space:]]*#Preview/ { skip = 1; depth = 0; open = 0 }
      skip { o = gsub(/\{/, "{"); c = gsub(/\}/, "}"); depth += o - c; if (o) open = 1
             if (open && depth <= 0) skip = 0; next }
      NF { n++ } END { print n + 0 }' {} \; | awk '{ s += $1 } END { print s + 0 }')
fi
echo "non-test lines: $n / $cap"
echo "apple lines: $m / $apple_cap"
test "$n" -le "$cap" && test "$m" -le "$apple_cap"
