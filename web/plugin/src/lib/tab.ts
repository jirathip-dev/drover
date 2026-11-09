// A tab module's whole life: herdsman's state through `view`, drawn in cooee's theme, or the error in its place.
import { state } from "./api.ts";
import { fail, render } from "./theme.ts";
import type { State } from "./types.ts";
import type { Section } from "./views.ts";

export function tab(view: (s: State) => Section[]): void {
  state().then((s) => render(view(s)), fail);
}
