A security advisory against `{fleet}`'s `{integration}` head, worked by a `deps` lane at once (#1488).

- Advisory: `{id}`
- Package: `{package}`
- Fixed in: {fixed}

1. Bump `{package}` (and what it needs to move with it) to the fixed version or later, in one PR.
2. Fix what breaks; add no dependency and remove none. No fixed version: say so in the PR body with the mitigation
   (an ignore in the advisory config with its reason is an owner ask).
3. The PR body carries a table: each dependency, its old and new version, and each lockfile's package count before
   and after.
