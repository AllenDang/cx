import assert from "node:assert/strict";
import test from "node:test";
import { summarize, checkExposure, isNavigation, pairedRows } from "./analyze.mjs";

test("counts model tool starts only, not warmups, partial output or final prose", () => {
  const stats = summarize([
    { type: "start" },
    { type: "tool_start", id: "a", name: "bash", args: { command: "python3 -m pytest -q" } },
    { type: "tool_start", id: "b", name: "cx_definition" },
    { type: "tool_end", id: "b", name: "cx_definition", isError: false, result: { details: { resultCount: 1 } } },
    { type: "tool_start", id: "c", name: "grep" },
    { type: "tool_start", id: "d", name: "cx_references" },
    { type: "tool_end", id: "d", name: "cx_references", isError: true },
    { type: "end", elapsedMs: 42 },
  ]);
  assert.equal(stats.calls, 4);
  assert.equal(stats.cxCalls, 2);
  assert.equal(stats.navigationCalls, 3);
  assert.equal(stats.firstNavigation, "cx_definition");
  assert.equal(stats.firstCxPosition, 2);
  assert.equal(stats.cxErrors, 1);
  assert.equal(stats.elapsedMs, 42);
});

test("empty runs do not fabricate denominators", () => {
  const stats = summarize([]);
  assert.equal(stats.cxShareNavigation, null);
  assert.equal(stats.cxShareAll, null);
  assert.equal(stats.firstCxPosition, null);
  assert.equal(stats.elapsedMs, null);
});

test("shell navigation classification is separate from tests and git", () => {
  assert.ok(isNavigation({ name: "bash", args: { command: "pwd; rg -n Cache src" } }));
  assert.ok(!isNavigation({ name: "bash", args: { command: "git diff --stat" } }));
  assert.ok(!isNavigation({ name: "edit" }));
});

test("exposure rejects wrong model, missing tools, changed schemas/guidance and contaminated tasks", () => {
  const baseline = [{ name: "cx_definition", description: "old", parameters: { type: "object" }, promptGuidelines: ["same"] }];
  const candidate = { cx_definition: "new" };
  const row = { model: "p/m", variant: "candidate", prompt: "Fix a bug." };
  const exposure = { model: "p/m", prompt: row.prompt, active: ["cx_definition"], tools: [{ ...baseline[0], description: "new" }] };
  assert.equal(checkExposure(row, exposure, baseline, candidate), null);
  assert.match(checkExposure(row, { ...exposure, model: "wrong" }, baseline, candidate), /model/);
  assert.match(checkExposure(row, { ...exposure, active: [] }, baseline, candidate), /inactive/);
  assert.match(checkExposure(row, { ...exposure, tools: baseline }, baseline, candidate), /description/);
  assert.match(checkExposure(row, { ...exposure, tools: [{ ...exposure.tools[0], parameters: {} }] }, baseline, candidate), /schema/);
  assert.match(checkExposure(row, { ...exposure, tools: [{ ...exposure.tools[0], promptGuidelines: [] }] }, baseline, candidate), /guideline/);
  assert.match(checkExposure({ ...row, prompt: "Use cx_definition" }, { ...exposure, prompt: "Use cx_definition" }, baseline, candidate), /leaks/);
});

test("paired analysis excludes the healthy partner of an invalid trial", () => {
  const rows = [
    { id: "a", model: "m", case: "one", variant: "baseline", valid: true },
    { id: "b", model: "m", case: "one", variant: "candidate", valid: false },
    { id: "c", model: "m", case: "two", variant: "baseline", valid: true },
    { id: "d", model: "m", case: "two", variant: "candidate", valid: true },
  ];
  assert.deepEqual(pairedRows(rows).map(r => r.id), ["c", "d"]);
});

test("only the exact native task/output wrapper is accepted", () => {
  const row = { id: "r01", model: "m", prompt: "Fix a bug." };
  const wrapped = `Task: ${row.prompt}\n\n---\n**Output:**\nWrite your findings to exactly this path: /artifacts/r01/result.md\nThis path is authoritative for this run.\nIgnore any other output filename or output path mentioned elsewhere, including output destinations in the base agent prompt, system prompt, or task instructions.`;
  const exposure = { model: "m", prompt: wrapped, tools: [], active: [] };
  assert.equal(checkExposure(row, exposure, [], {}), null);
  assert.match(checkExposure(row, { ...exposure, prompt: wrapped + "\nUse a particular tool." }, [], {}), /prompt/);
  assert.match(checkExposure(row, { ...exposure, prompt: wrapped.replace("/r01/", "/r02/") }, [], {}), /binding/);
});
