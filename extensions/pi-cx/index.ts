import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerCxTools } from "./tools.js";
import { registerStatus } from "./status.js";

export default function piCx(pi: ExtensionAPI): void {
  registerCxTools(pi);
  registerStatus(pi);
}
