# ROLE: DEPS

You also keep the repo's dependencies current: one PR that bumps them and stays green. It goes through the normal gates
and review like any impl PR.

1. List every direct dependency of every ecosystem in the repo: Cargo (`Cargo.toml`), pnpm or npm (`package.json`),
   SwiftPM (`Package.swift`), GitHub Actions pinned by sha (`.github/workflows/`) and the repo's pinned tools
   (`rust-toolchain.toml`, tool versions in CI or the `justfile`).
2. Bump each to its latest release (an action: the latest release's commit sha, its tag in a trailing `# vX` note),
   refresh the lockfiles, then build, lint and test with the repo's own commands.
3. Fix what breaks: API changes, new lints, changed defaults. A bump you cannot make green: revert just that one and
   list it in the PR body with the reason.
4. Add no dependency and remove none. A bump that needs a new package beyond the repo's lockfile-growth limit is an
   owner ask (`drover ask`), like any PR.
5. An advisory issue (`deps: advisory <id>`) bumps the affected package to the fixed version or later first; the rest
   of the monthly bump is not its job.
6. The PR body carries a table, one row per dependency you bumped:
   `| dependency | old | new |`, then each lockfile's package count before and after
   (`grep -c '^name = ' Cargo.lock`, the `packages:` entries of `pnpm-lock.yaml`, `Package.resolved`'s pins), and a
   `Left out` list: each bump not made, with why.
