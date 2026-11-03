# 15,670 → 16,500 formatted lines: migration ceiling (#309)

#309 (an owner extend grants the next round) is `src/` net +3, which put staging's 15,668 one line over the
15,670 cap. Instead of a per-PR raise, the owner set one temporary ceiling.

Owner pick (2026-11-03, ask q162), verbatim:

> One temporary ceiling now, then the real cut: raise line_cap once to 16,500 as a migration ceiling (recorded in
> cap-history, expiring when #270 lands), so lanes stop asking per PR; move the decoupling chain (#264-#270) up right
> after the in-flight work so the HTML leaves Rust sooner; #270 then resets the cap to measured +5% (expected ~11-12k).

This ceiling expires when #270 lands: #270 resets the cap to the measured size +5%.
