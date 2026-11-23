# Limits (#844)

Every timing and cap drover core uses is a `[limits]` key; a fleet.toml without one keeps the default below.
`drover config show` prints each key with its current value and this doc line, and CI's timing check
(`scripts/timing-check.sh`) fails a hard-coded `Duration` in `crates/core/src` outside `config.rs` and tests.
This file is `drover config show --md`'s output: `just gen` writes it.

| key | default | what it sets |
| --- | --- | --- |
