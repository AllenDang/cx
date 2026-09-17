import { appendFileSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerCxTools } from "../../extensions/pi-cx/tools.js";
import { registerDirtyPathProtocol } from "../../extensions/pi-cx/dirty-paths.js";

/** No model calls, tool-choice nudges or result rewrites: metadata treatment + observation only. */
export function studyExtension(pi: ExtensionAPI, root: string): void {
  const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
  const baseline = JSON.parse(readFileSync(join(root, "baseline.json"), "utf8"));
  const candidate = JSON.parse(readFileSync(join(root, "candidate.json"), "utf8"));
  let record: any;
  let started = 0;
  const log = (data: any) => {
    if (!record) throw new Error("Study context not initialized");
    appendFileSync(join(record.evidence, "events.jsonl"), JSON.stringify({ at: Date.now(), ...data }) + "\n");
  };
  // Registration happens before the session context exists. Select treatment at session_start.
  const registered: any[] = [];
  const capture = { registerTool: (tool: any) => registered.push(tool) } as unknown as ExtensionAPI;
  const dirty = registerDirtyPathProtocol(pi);
  registerCxTools(capture, dirty);
  for (const tool of registered) pi.registerTool(tool);
  pi.on("session_start", (_event, ctx) => {
    record = manifest.records.find((r: any) => r.cwd === realpathSync(ctx.cwd));
    if (!record) throw new Error(`Unknown study cwd: ${ctx.cwd}`);
    for (const tool of registered) {
      const frozen = baseline.find((t: any) => t.name === tool.name);
      if (!frozen || JSON.stringify(tool.parameters) !== JSON.stringify(frozen.parameters)) throw new Error("Schema drift");
      pi.registerTool({ ...tool, ...frozen,
        description: record.variant === "candidate" ? candidate[tool.name] : frozen.description });
    }
  });
  pi.on("before_agent_start", (event, ctx) => {
    started = Date.now();
    writeFileSync(join(record.evidence, "exposure.json"), JSON.stringify({
      active: pi.getActiveTools(), tools: pi.getAllTools(), systemPrompt: event.systemPrompt,
      model: ctx.model && `${ctx.model.provider}/${ctx.model.id}`, thinking: ctx.thinkingLevel,
      prompt: event.prompt,
    }, null, 2));
    log({ type: "start" });
  });
  pi.on("tool_execution_start", event => log({ type: "tool_start", id: event.toolCallId, name: event.toolName, args: event.args }));
  pi.on("tool_execution_end", event => log({ type: "tool_end", id: event.toolCallId, name: event.toolName, isError: event.isError, result: event.result }));
  pi.on("message_end", event => {
    if (event.message.role === "assistant") {
      const message = event.message as any;
      log({ type: "assistant_end", usage: message.usage, stopReason: message.stopReason, errorMessage: message.errorMessage });
    }
  });
  pi.on("agent_end", () => log({ type: "end", elapsedMs: Date.now() - started }));
}
