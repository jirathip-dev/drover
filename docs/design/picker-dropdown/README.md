## Summary

Decision: every picker on the phone page becomes one bracketed control, `[model sonnet-5-5 ▾]`, that opens a square, framed list in the page's own font and colours (no blur, no rounded system sheet). The current value is marked `*` in green, and the row the finger or arrow keys are on is filled in the accent colour, as the active tab is.
Screens: the thread composer with its model list opening upward above the chips, and the Fleets settings form with a list opening downward under its field, each at 390 and 1440.
Open questions: none. Two calls I made that you can change: there is no dimmed backdrop (a tap outside still closes the list), and long options wrap onto a second line instead of being cut off.

## Images

| Image | What it shows |
|---|---|
| `composer-390.png` | The composer's **model** chip open. The list opens **upward** because the composer sits at the foot of the screen, starts at the chip's left edge and is at least as wide as the chip. The `model` title sits in the frame's top edge. `* sonnet-5-5` is the current value, and the list opens with it as the active row. The closed **effort** chip next to it shows the closed state: `[effort high ▾]`, its name muted and its value in the text colour. |
| `composer-1440.png` | The same in the 760 px focus column at desktop width: the chips share the row as the selects do today, and the list keeps the chip's width. |
| `form-390.png` | The Fleets sheet's **Impl** picker open **downward**, under its field. In a form, the list lines up with the field's **right** edge and grows leftward, so a long value such as `Claude Code · opus 5.5 · high` stays on screen at 390. The current value is `*` and green, and the arrow keys have moved down one row: the active row is filled in the accent colour and is not yet picked. Closed pickers (Host, Review, Rounds) look like the page's other bracket controls, with `▾` at the right. |
| `form-1440.png` | The same sheet at 1440: the list is as wide as the field. |

## The control (for impl)

- **One component** in `web/src`. It replaces both `el('select')` sites: the composer chips (`blocks.ts:384`) and form `picker` fields (`blocks.ts:459`). `values()` reads its value from a hidden input that has the field's `name`, so the form posts exactly what it posts today.
- **Trigger:** a `button` with the page's `[ ]` chrome (`aria-haspopup=listbox`, `aria-expanded`). It holds the value, with the chip's name before it in muted text, and a `▾` that turns to `▴` while open. The open trigger turns the accent colour.
- **Panel:** `.ddp`, square and framed with `--edge` on `--bg`, an 8 px `--bg` gutter around it so it stands clear of the cards behind, and its title (`.ddh`) set into the top border. Inside, a `ul role=listbox` with `aria-activedescendant` scrolls when the space runs out. Each option is a `li role=option` at least 44 px tall. The current value has `aria-selected=true`, a `*` gutter and green text. The active row (`.on`) is filled with `--l`.
- **Direction and fit:** the list opens upward when there is less room below the trigger than above it, which is always the case for the composer chips. Its height is capped at the space on that side (`--room`) and it scrolls past that. Chips line up on their left edge and form fields on their right edge. Width is capped at the viewport minus the page's margins, so the list never leaves the screen at 390.
- **Behaviour:** a tap opens and closes the list. Tapping an option picks it and closes the list: the composer posts the same `model` action, or redraws the same way, as `onchange` does today. A tap outside or Esc closes the list without picking, and focus returns to the trigger. The arrow keys, Home and End move the active row, Enter or Space picks it, and Tab closes the list.
- `dd.css` holds the proposed rules; it uses tokens only and moves into `web/src/board.css`. `tokens.css` is mockup-only: it sets the github-dark scheme and the 760 px column.

## Files

`composer.html` and `form.html` link the real `web/src/board.css`, then `tokens.css` and `dd.css`. To redo the shots: `sh docs/design/picker-dropdown/shoot.sh` (Playwright's chrome-headless-shell; you can pass a path to another one).
