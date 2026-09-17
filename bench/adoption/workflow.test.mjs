import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const script = readFileSync(new URL("./workflow.js", import.meta.url), "utf8");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const execute = new AsyncFunction("args", "runs", "emit", script);
const records = Array.from({ length: 18 }, (_, i) => ({ id: `r${i + 1}`, case: 'task', model: 'p/m', cwd: `/trial/${i}`, prompt: 'Fix a bug.' }));
function jsonOnly(value) {
  assert.notEqual(value, undefined);
  if (value && typeof value === 'object') Object.values(value).forEach(jsonOnly);
}

test("workflow emits JSON even when optional child result fields are missing", async () => {
  let launches = 0;
  const result = await execute({ records }, { all: async items => {
    assert.equal(items.length, 3);
    launches += items.length;
    return items.map(() => ({ ok: true, runId: 'test', outputReference: '/output' }));
  } }, jsonOnly);
  jsonOnly(result);
  assert.equal(launches, 18);
  assert.equal(result.results.length, 18);
  assert.equal(result.blocked, false);
  assert.equal(result.results[0].outputPathMapping, null);
});

test("workflow stops after a failed wave without dispatching later trials", async () => {
  let launches = 0;
  const result = await execute({ records }, { all: async items => {
    launches += items.length;
    return [{ ok: true }, { ok: false }, { ok: true }];
  } }, jsonOnly);
  assert.equal(launches, 3);
  assert.equal(result.blocked, true);
});
