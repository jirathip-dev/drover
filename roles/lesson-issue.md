This week's lessons for `{fleet}`, worked by a `lesson` lane (#419).

1. Read `drover lesson digest {fleet} --days 7`.
2. Group it into mistake classes; a class counts once it happened twice in the window.
3. Fix each class at the highest level that works: architecture or types, then a lint or CI check whose error names
   the fix, then a behaviour test, and a "don't X" line in `.drover/rules.md` last.
4. Prove each new check fails on a real past mistake from the digest: the command and its output go in the PR body.
5. No `.drover/rules.md` yet: this run creates it. It stays at most 30 lines, each naming its enforcer (or `none`); a rule code now enforces is
   deleted in the same PR; a `none` rule older than 2 weeks is retired unless promoted. The PR adds or changes at
   least one enforcer: a prose-only one is a FAIL in rev unless it retires lines.
6. New checks start in shadow (`shadow since <date>` on their rules line); `drover lesson apply {fleet}` promotes or
   retires the existing ones by their history (#510). Hand-written rules lines are never changed.

No class qualifies: `drover done --no-change "no repeated class" {fleet} <this issue>`; drover closes this issue.
