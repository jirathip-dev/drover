// The drover-queue tab (cooee plugin module): the queue drawn in cooee's theme; a sideways swipe or ←/→ switches repo.
import { mount } from "./lib/dom.ts";
import { CSS, queueTab } from "./lib/queue.ts";

const t = queueTab((kids) => mount(kids, CSS));
t.load();
// A swipe over 40 px, more across than down; the click that ends it opens nothing.
let from: { x: number; y: number } | null = null,
  swiped = false;
addEventListener("pointerdown", (e) => {
  from = (e.target as Element).closest("input, select") ? null : { x: e.clientX, y: e.clientY };
  swiped = false;
});
addEventListener("pointerup", (e) => {
  const dx = from ? e.clientX - from.x : 0,
    dy = from ? e.clientY - from.y : 0;
  from = null;
  if (Math.abs(dx) > 40 && Math.abs(dx) > 2 * Math.abs(dy)) {
    swiped = true;
    t.step(-Math.sign(dx));
  }
});
addEventListener(
  "click",
  (e) => {
    if (!swiped) return;
    e.preventDefault();
    e.stopPropagation();
    swiped = false;
  },
  true,
);
addEventListener("keydown", (e) => {
  const k = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
  if (k && !(e.target as Element).closest?.("input, select")) t.step(k);
});
