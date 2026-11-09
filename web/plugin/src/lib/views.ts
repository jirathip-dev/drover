// Each tab's view of herdsman's state: titled lists of rows, drawn by `theme.render`.
import { laneHref } from "./links.ts";
import type { State } from "./types.ts";

export type Item = { text: string; tone?: string; href?: string };
export type Section = { title: string; items: Item[] };

/** drover-lanes: each lane, as its Lanes row says it, under its role (impl, rev, verify, …) in order of appearance. */
export const lanesView = (s: State): Section[] =>
  [...new Set(s.lanes.map((l) => l.role))].map((role) => ({
    title: role,
    items: s.lanes
      .filter((l) => l.role === role)
      .map((l) => ({ text: `${l.name} · ${l.shown}`, tone: l.tone, href: laneHref(l.name) })),
  }));
