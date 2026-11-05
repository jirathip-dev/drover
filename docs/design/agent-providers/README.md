# Agent providers: design (#496, shared by #497 and #498)

`index.html` is the ⚙ System sheet (#440) open at **Agent providers**, with its Anthropic and DeepSeek cards, plus
the flows their buttons start. It links `assets/serve.css` and `herd.css` unchanged (farm scheme, as the export).
One `proposed` `<style>` block holds the only new rules (existing tokens only); they move to `serve.css` in impl.
The markup comes from small functions that extend `web/src/system.ts` `providers()` / `account()`, so impl can port
them as they are.

| view | what | shots |
|---|---|---|
| `index.html` | the section: every row state | `providers-1440.png`, `providers-390.png` |
| `#signin` | [+ add account] / [sign in again] → setup terminal → token saved (#496) | `signin-390.png`, `signin-1440.png` |
| `#use` | [use] scope → "move N running lanes" → a conductor's proposal card (#498) | `use-390.png`, `use-1440.png` |
| `#forms` | [+ add key] valid / refused (#497), [remove]'s confirm, a viewer device | `forms-390.png`, `forms-1440.png` |

`node shoot.mjs <chrome-headless-shell>` re-shoots all eight (390 at DPR 3) and measures each 390 view by
`ui-audit.js` `measure()`'s rules: 28 px controls with 44 px tap areas, 11/13/16 px text, no text past its box.
All four views pass (`{"providers":[],"signin":[],"use":[],"forms":[]}`).

**Data:** fixtures. Accounts A, B, C in that order for every role; the 5h/7d percentages, reset times, lane-hours,
fleet names, issue numbers and the DeepSeek balance are illustrative.

## The Anthropic card

```
Anthropic  Claude · setup tokens              [+ add account]
A [1st]                           capped until 16:40   ← amber; badge amber (first in order, but capped)
used by nobody while capped · its roles fall through to B
5h ▕████████████▏ 100%   7d ▕████████░░░░▏ 64%       ← #258 status line, the bars the host card uses
status line 4m ago · 5h resets 16:40 · 7d resets Fri 11:59 · 6.2 lane-h today   ← usage age first
[sign in again]                               [remove]
B [active]                                         ok   ← green filled badge: the account new lanes take
used by impl · rev · verify · conductor · every fleet
…
[use]  [sign in again]                        [remove]
▌C [3rd]                  token invalid: sign in again   ← red, the row's red left bar (data-t=r)
▌used by nobody · skipped until it signs in again
▌refused 2h ago (401 on spawn) · no usage since Mon       ← no bars when there is no reading
▌[sign in again]                              [remove]   ← [sign in again] bold red; no [use]
```

- **Badge:** `active` (filled green) on the first uncapped, valid account in the default order, else its place:
  `1st`, `2nd`, `3rd`… (`.tag`; amber when capped). One badge per row, never both.
- **State** (right, in its tone): `ok` · `capped until <time>` · `token invalid: sign in again`.
- **Who uses it:** the roles whose first live account this is; a per-fleet override reads `atlas impl`.
- **Usage age** opens the meta line (`status line 4m ago`), so an old reading reads as old.
- **Buttons:** `[use]` (#498) shows unless the account is already first for every role, or is invalid;
  `[sign in again]`; `[remove]` red, right. All of them only on a paired owner device (`rw`); a viewer sees the rows
  and no buttons, and the head's `[+ add …]` goes too.

## Sign-in (`#signin`)

1. `[+ add account]` opens an inline step under the card head: a name field prefilled with the next free letter,
   `[sign in]` `[cancel]`. `[sign in again]` skips it (the row's name).
2. The lane sheet opens over System as #303's setup terminal on the **main host**: the masked screen, an `[open]` per
   printed URL, the reply box (`paste the code`) for the code the browser shows.
3. When `claude setup-token` prints the token, the server reads it from the pane, saves it through #441's store
   and clears the pane before the next poll answers. The sheet only ever shows the green capture line
   (`✓ saved · Anthropic C's token is in the keychain · the pane is cleared · …`). The token is never in a poll
   response, event, log or capture.
4. Only then does the row appear (`used by nobody yet · 3rd for every role`, `usage shows after its first lane`).
- ✗: no token printed (closed, timed out, refused): a red capture line with setup-token's last error,
  `nothing changed`, and `[try again]`.

Adds one rule to the setup terminal: `pre.out` wraps (`pre-wrap`, `anywhere`), as `.out pre` does elsewhere.
Today a login URL runs past 390 px.

## [use] (`#use`, #498)

An inline step under the row: `use C first for` → `[every role · all fleets]` (a full-width `.pick`), or
`one fleet's role [atlas ▾] [impl ▾] [use there]`, and `[cancel]`. After the choice, the same box says
`✓ C is 1st for every role · new lanes use it now`. When lanes run on another account it adds one prompt:
`3 running lanes are on B: …` `[move 3 running lanes]` `[keep them]`, with the note that each finishes its turn and
resumes the same session (a harness that can't resume gets a handover). A conductor's proposal is a thread block
like the config card (`proposes use C for every role`, `[apply]` `[discard]`). `[apply]` runs [use] with that scope,
then the same lanes prompt.

## DeepSeek and [remove] (`#forms`, #497)

`[+ add key]`: an inline step with `name`, then the how-to line with `[open]` to DeepSeek's API key page, then the
masked `key` field (`type=password`), `[check and save]` `[cancel]`. A refused key keeps the step open, the field
red, and says why (`✗ DeepSeek said 401 Authentication Fails · nothing saved`). A key row shows `✓ ¥ 42.10` (green)
or `invalid: replace it` (red, the red bar, `[replace]` bold), who uses it and under which env var, and
`checked 12m ago · rechecked hourly`.

`[remove]` opens the red confirm box (`.cf`'s frame) under the row: `remove C? its token leaves the keychain ·
roles that list it skip it · <who still uses it>`, `[remove]` `[cancel]`.

Button labels never carry an account name: buttons are lowercased by `serve.css`, and `[keep them on b]` reads wrong.

## Open questions for the owner

1. **Name on add:** the step prefills the next free letter and lets you type another name. Or should it be letters
   only, with no field at all?
2. **DeepSeek order:** keys have no `[use]` here (roles name a key). Should DeepSeek keys get an order and `[use]`
   like accounts?
3. **[remove] with live lanes:** the design lets you remove an account that running lanes still use (the confirm
   names them, and they keep their token until they end). Or should it refuse until they move?
