import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerCxTools } from "./tools.js";
import { registerStatus } from "./status.js";

export default function piCx(pi: ExtensionAPI): void {
  registerCxTools(pi);
  registerStatus(pi);
  const names = ["cx_overview", "cx_symbols", "cx_definition", "cx_references", "cx_callers", "cx_callees", "cx_map", "cx_refresh"];
  pi.setActiveTools([...new Set([...pi.getActiveTools(), ...names])]);
}
