# Merge label (#947, #1876)

`merge_label` is the PR label the full CI needs. It defaults to `"merge-ready"` on every fleet; `merge_label = "off"`
opts a fleet out (no label, no wait for a full run). drover adds it on a PASS at the head and removes it when the head
moves past that PASS. `drover setup` creates it in the repo, and `drover doctor` checks that it exists. Pair it with
a CI that runs light gates on an unlabelled PR and the full set on the label: [ci.md](../ci.md).
