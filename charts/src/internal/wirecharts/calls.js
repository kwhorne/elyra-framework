// Each WireCharts component's call, as its Blade view makes it (RFC 0059):
// the props, its defaults filled in, to the builder's arguments.
import { CALLS as LINE } from "./line.js";
import { CALLS as AREA } from "./area.js";
import { CALLS as MIXED } from "./mixed.js";
import { CALLS as COLUMNS } from "./columns.js";
import { CALLS as MORE } from "./more.js";

export const CALLS = { ...LINE, ...AREA, ...MIXED, ...COLUMNS, ...MORE };
