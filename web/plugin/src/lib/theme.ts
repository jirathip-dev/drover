// cooee's theme: its CSS variables are set on the frame's root, so the tabs only name them.
import type { Section } from "./views.ts";

const CSS = `
h2{font-size:1em;color:var(--mute);margin:16px 0 6px}
ul{list-style:none;margin:0;padding:0}
li{background:var(--card);border:1px solid var(--edge);border-radius:8px;padding:8px 10px;margin:4px 0}
a{color:var(--link)}
.g{color:var(--g)}.a{color:var(--a)}.r{color:var(--r)}.m{color:var(--mute)}
`;

function el(tag: string, text: string, cls = ""): HTMLElement {
  const e = document.createElement(tag);
  e.textContent = text;
  e.className = cls;
  return e;
}

/** Draws `sections` as the tab's body in cooee's colours: a tone (g, a, r, m) colours a row, an `href` links it. */
export function render(sections: Section[]): void {
  document.head.replaceChildren(el("style", CSS));
  document.body.replaceChildren(
    ...sections.flatMap(({ title, items }) => {
      const ul = document.createElement("ul");
      for (const { text, tone, href } of items) {
        const row = href ? Object.assign(el("a", text, tone), { href, target: "_top" }) : el("span", text, tone);
        ul.appendChild(document.createElement("li")).appendChild(row);
      }
      return [el("h2", title), ul];
    }),
  );
}

/** A tab's failure, shown in the tab rather than as a blank frame. */
export function fail(e: unknown): void {
  document.body.replaceChildren(el("p", String(e), "r"));
}
