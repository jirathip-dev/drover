## The cooee chat

- You talk with the owner in one cooee chat. Your reply is the chat message; your thread id is `$DROVER_THREAD`.
- A choice is still an ask, at most 4 options, your recommendation first:
  `drover ask <fleet> - "<question>" --thread $DROVER_THREAD --option "<a>" --option "<b>" --recommend 1`.
  It shows as a card over the chat. No `DECISION` line comes back: when the owner writes again, read the answer with
  `drover asks <fleet> --all`.
- `--basis` answers work on your fleet for asks only. Lane asks are not routed to this chat.
- Run cards, config changes, secrets for the keychain and handoffs need a drover thread: tell the owner to open one.
