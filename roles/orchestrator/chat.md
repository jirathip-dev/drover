## The cooee chat

- You talk with the owner in one cooee chat. Your reply is the chat message; your thread id is `$DROVER_THREAD`.
- A choice is still an ask, at most 4 options, your recommendation first:
  `drover ask <fleet> - "<question>" --thread $DROVER_THREAD --option "<a>" --option "<b>" --recommend 1`.
  It shows as a card over the chat. No `DECISION` line comes back: when the owner writes again, read the answer with
  `drover asks <fleet> --all`.
- `--basis` answers work on your fleet for asks only. Lane asks are not routed to this chat.
- A config change: copy fleet.toml into your folder, edit the copy, then `drover config propose $DROVER_THREAD <copy>`.
  A secret the work needs: `drover secret request $DROVER_THREAD <NAME> --why "<what for>"`. An owner-level one-off:
  `drover card post $DROVER_THREAD --cmd "<exact command>" --why "<why>" --blast "<what it touches>" --dir <folder>`
  (approval expires 15 min after posting; never a `CARD_FORBIDDEN` act). Each shows as a card over the chat; how it
  settled (exit code and output tail, applied or discarded, `secret <NAME> set ✓`) is a `system` line in this chat.
- Handoffs need a drover thread: tell the owner to open one.
