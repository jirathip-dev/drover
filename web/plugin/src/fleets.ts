// The drover-fleets tab (cooee plugin module): each fleet's config, pipeline and roles, the hosts lanes run on, fleet
// disk, notifiers and activity, drawn in cooee's theme.
import { mount } from "./lib/dom.ts";
import { CSS, fleetsTab } from "./lib/fleets.ts";

fleetsTab((kids) => mount(kids, CSS)).load();
