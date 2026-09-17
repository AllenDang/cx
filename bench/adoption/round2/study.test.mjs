import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { resolveMetadata, inspectTrial } from './lib.mjs';
const baseline = JSON.parse(readFileSync(new URL('../baseline.json', import.meta.url)));
const presets = JSON.parse(readFileSync(new URL('./presets.json', import.meta.url)));

test('presets only alter model-facing metadata and keep tool order, schemas and boundaries', () => {
  for (const name of Object.keys(presets)) {
    const result = resolveMetadata(baseline, presets, name);
    assert.deepEqual(result.map(t => t.name), baseline.map(t => t.name));
    for (let i = 0; i < result.length; i++) {
      assert.deepEqual(result[i].parameters, baseline[i].parameters);
      assert.ok(result[i].description);
      assert.ok(result[i].promptGuidelines.every(g => /cx_/.test(g)));
      assert.ok(result[i].promptSnippet);
    }
  }
  assert.deepEqual(resolveMetadata(baseline, presets, 'baseline'), baseline);
});

test('empty results, overview-only, error and post-edit queries do not earn source adoption', () => {
  const record = { id: 's01', model: 'm', prompt: 'Fix code.', metadata: [] };
  const exposure = { model: 'm', prompt: 'Task: Fix code.\n\n---\n**Output:**\n', tools: [], active: [], systemPrompt: '' };
  const pair = (name, count, error = false) => [
    { type: 'tool_start', id: name, name },
    { type: 'tool_end', id: name, name, isError: error, result: { details: { resultCount: count } } },
  ];
  const inspect = items => inspectTrial(record, exposure, [{ type: 'start' }, ...items, { type: 'end' }]);
  assert.equal(inspect(pair('cx_overview', 4)).meaningfulAdoption, false);
  assert.equal(inspect(pair('cx_definition', 0)).meaningfulAdoption, false);
  assert.equal(inspect(pair('cx_definition', 1, true)).meaningfulAdoption, false);
  assert.equal(inspect([{ type: 'tool_start', name: 'edit' }, ...pair('cx_definition', 1)]).meaningfulAdoption, false);
  assert.equal(inspect(pair('cx_definition', 1)).meaningfulAdoption, true);
  assert.ok(inspect([{ type: 'assistant_end', stopReason: 'error' }]).issues.includes('provider/runtime error'));
});

test('edit tasks contain no tool directives', () => {
  const heldout = JSON.parse(readFileSync(new URL('./heldout.json', import.meta.url)));
  const old = JSON.parse(readFileSync(new URL('../cases.json', import.meta.url)));
  for (const task of [...heldout, ...old.cases]) assert.doesNotMatch(task.prompt + old.suffix, /\bcx\b|cx_/i);
});

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const runWorkflow = new AsyncFunction('inventory', 'studyRoot', 'judgeScript', 'runs', 'emit', readFileSync(new URL('./workflow.js', import.meta.url), 'utf8'));
const inventory = [{ id: 'control', phase: 'baseline' }, ...Object.keys(presets).flatMap(arm =>
  ['development', 'validation'].map(phase => ({ id: arm + phase, arm, phase })))];

test('workflow advances on non-adoption, validates a candidate and stops before stronger policies', async () => {
  const keys = [];
  const runs = {
    all: async rows => { keys.push(...rows.map(r => r.key)); return rows.map(() => ({ ok: true })); },
    run: async key => { keys.push(key); return { ok: true, structuredOutput: {
      pass: !key.startsWith('compact'), infrastructure: false, meaningful: key === 'baseline-check' ? 0 : 2,
    } }; },
  };
  const result = await runWorkflow(inventory, '/study', '/judge.mjs', runs, value => assert.doesNotThrow(() => JSON.stringify(value)));
  assert.equal(result.winner, 'bilingual');
  assert.ok(!keys.some(key => key.startsWith('priority')));
  assert.ok(!keys.includes('compactvalidation'));
});

test('workflow stops on infrastructure failure rather than trying another policy', async () => {
  const result = await runWorkflow(inventory, '/study', '/judge.mjs', {
    all: async rows => rows.map(() => ({ ok: true })),
    run: async () => ({ ok: true, structuredOutput: { infrastructure: true, pass: false } }),
  }, () => {});
  assert.equal(result.blocked, true);
  assert.equal(result.reason, 'baseline evidence failure');
});
