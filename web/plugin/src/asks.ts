// The drover-asks tab (cooee plugin module): the needs-you cards drawn in cooee's theme, read again on each herdsman
// event (#17) and every 15 s; a permission prompt's countdown ticks each second.
import { asksTab, CSS } from "./lib/asks.ts";
import { mount } from "./lib/dom.ts";

const t = asksTab((kids) => mount(kids, CSS));
t.load();
t.listen();
setInterval(t.tick, 1000);
setInterval(t.load, 15000);
