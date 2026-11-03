# 16,500 → 17,000 formatted lines: migration ceiling, raised once more (#294)

#294 (named serial groups) is `src/` net +214, which put the branch at 16,680 on top of staging's 16,466, over the
16,500 migration ceiling.

Owner pick (2026-11-03, ask q164), verbatim:

> Raise the migration ceiling once more to 17,000 (same rule: recorded in cap-history, ends when #270 resets the cap).
> Both land as built: #294 keeps auto-join and holds (+214), and #316 lands with the generic memory row its spec asks
> for (+~95). The decoupling chain (#265-#270) is next in line right after these, so the cut comes soon

This ceiling expires when #270 lands: #270 resets the cap to the measured size +5%.
