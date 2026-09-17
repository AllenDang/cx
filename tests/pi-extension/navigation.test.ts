import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import piCx from "../../extensions/pi-cx/index.js";
import { appendSourceNavigationGuidance, SOURCE_NAVIGATION_GUIDANCE } from "../../extensions/pi-cx/navigation.js";
import { registerCxTools } from "../../extensions/pi-cx/tools.js";

const entryTools = ["cx_symbols", "cx_definition", "cx_context"];

test("source guidance exactly matches the measured confirmation policy", () => {
  const policy = JSON.parse(readFileSync(new URL("../../bench/adoption/round4/policy.json", import.meta.url), "utf8"));
  assert.equal(SOURCE_NAVIGATION_GUIDANCE, policy.text);
  assert.match(SOURCE_NAVIGATION_GUIDANCE, /documentation-only, configuration-only/);
  assert.match(SOURCE_NAVIGATION_GUIDANCE, /ordinary tools to continue/);
});

test("source guidance preserves existing instructions and is idempotent at the tail", () => {
  const original = "Existing system and project instructions.";
  const active = Object.freeze([...entryTools, "read", "bash"]);
  const appended = appendSourceNavigationGuidance(original, active);
  assert.equal(appended, original + "\n\n" + SOURCE_NAVIGATION_GUIDANCE);
  assert.equal(appendSourceNavigationGuidance(appended, active), appended);
});

test("source guidance never directs the model to an inactive entry tool", () => {
  for (const missing of entryTools) {
    assert.equal(appendSourceNavigationGuidance("original", entryTools.filter(name => name !== missing)), "original");
  }
  assert.equal(appendSourceNavigationGuidance("original", []), "original");
});

test("extension wires tool-side system guidance without rewriting user requests or tool availability", () => {
  const handlers = new Map<string, Function>();
  let active = [...entryTools, "read", "bash"];
  const api: any = {
    registerTool() {}, registerCommand() {}, registerEntryRenderer() {},
    events: { on() { return () => {}; } },
    on(name: string, handler: Function) { handlers.set(name, handler); },
    getActiveTools() { return active; },
    setActiveTools() { throw new Error("guidance must not force the tool set"); },
  };
  piCx(api);
  const event = { systemPrompt: "original", prompt: "Fix a bug." };
  assert.deepEqual(handlers.get("before_agent_start")!(event, {}), { systemPrompt: "original\n\n" + SOURCE_NAVIGATION_GUIDANCE });
  assert.deepEqual(event, { systemPrompt: "original", prompt: "Fix a bug." });
  active = ["read", "bash"];
  assert.equal(handlers.get("before_agent_start")!(event, {}), undefined);
});

test("all registered metadata matches the winning snapshot and schemas stay unchanged", () => {
  const baseline = JSON.parse(readFileSync(new URL("../../bench/adoption/baseline.json", import.meta.url), "utf8"));
  const presets = JSON.parse(readFileSync(new URL("../../bench/adoption/round2/presets.json", import.meta.url), "utf8"));
  const expected = baseline.map((tool: any) => ({ ...tool, ...presets.compact.tools[tool.name], ...presets.priority.tools[tool.name] }));
  const actual: any[] = [];
  registerCxTools({ registerTool(tool: any) {
    const { name, description, parameters, promptSnippet, promptGuidelines } = tool;
    actual.push(JSON.parse(JSON.stringify({ name, description, parameters, promptSnippet, promptGuidelines })));
  } } as any);
  assert.deepEqual(actual, expected);
});
