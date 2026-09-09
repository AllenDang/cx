import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerCxTools } from "./tools.js";
import { registerStatus } from "./status.js";
import { registerDirtyPathProtocol } from "./dirty-paths.js";

export default function piCx(pi: ExtensionAPI): void {
  const dirty = registerDirtyPathProtocol(pi);
  registerCxTools(pi, dirty);
  registerStatus(pi);
}
