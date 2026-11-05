This week's lessons for `{fleet}`, worked by a `lesson` lane (#419).

1. Read `herdsman lesson digest {fleet} --days 7`.
2. Group it into mistake classes; a class counts once it happened twice in the window.
3. Fix each class at the highest level that works: architecture or types, then a lint or CI check whose error names
   the fix, then a behaviour test, and a "don't X" line in `.herdsman/rules.md` last.
4. Prove each new check fails on a real past mistake from the digest: the command and its output go in the PR body.
5. `.herdsman/rules.md` stays at most 30 lines, each naming its enforcer (or `none`); a rule code now enforces is
   deleted in the same PR.

No class qualifies: `herdsman done --no-change "no repeated class" {fleet} <this issue>`; herdsman closes this issue.
