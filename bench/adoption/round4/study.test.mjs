import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { possibleShellMutation, inspectTrial } from './lib.mjs';

test('stderr and discarded stdout are not mistaken for source mutation', () => {
  assert.equal(possibleShellMutation('cat source.py 2>/dev/null | head -150'), false);
  assert.equal(possibleShellMutation('cat setup.cfg >/dev/null'), false);
  assert.equal(possibleShellMutation('cat > src/new.py <<EOF\npass\nEOF'), true);
  assert.equal(possibleShellMutation("p.write_text('changed')"), true);
});

test('successful source returned before a direct edit earns adoption despite stderr redirection', () => {
  const record = { cwd: '/repo', model: 'm', prompt: 'Fix code.', metadata: [] };
  const exposure = { model: 'm', prompt: 'Task: Fix code.\n\n---\n**Output:**\n', tools: [], active: [], systemPrompt: '' };
  const events = [
    { type: 'start' },
    { type: 'tool_start', id: 'shell', name: 'bash', at: 1, args: { command: 'cat source.py 2>/dev/null' } },
    { type: 'tool_start', id: 'lookup', name: 'cx_definition', at: 2 },
    { type: 'tool_end', id: 'lookup', name: 'cx_definition', at: 3, isError: false, result: { details: { resultCount: 1 } } },
    { type: 'tool_start', id: 'edit', name: 'edit', at: 4, args: { path: 'src/source.py' } },
    { type: 'end' },
  ];
  assert.equal(inspectTrial(record, exposure, events).meaningfulAdoption, true);
  assert.equal(inspectTrial(record, exposure, events).requiresManualMutationAudit, false);
  const concurrent = structuredClone(events);
  concurrent[4].at = 2;
  assert.equal(inspectTrial(record, exposure, concurrent).meaningfulAdoption, false);
});

test('policy explicitly keeps ordinary tools and document/config exceptions', () => {
  const policy = JSON.parse(readFileSync(new URL('./policy.json', import.meta.url)));
  assert.match(policy.text, /ordinary tools/);
  assert.match(policy.text, /documentation-only/);
  assert.match(policy.text, /configuration-only/);
  assert.match(policy.kind, /explicit/);
});
