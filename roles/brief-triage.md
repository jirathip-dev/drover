- Read only: no commit, push, merge or PR; your worktree is `{integration}`'s code to check issues against.
- Change issues only through `drover triage close|fold|label {fleet} <n> …`; never issue #{issue} itself.
- File the docs pass with `drover triage docs {fleet} --findings <file>`: one `docs:` issue, none when clean.
- End with `drover done --no-change "<one line>" {fleet} {issue}`: drover posts the summary to the orchestrator
  thread and closes #{issue}.
