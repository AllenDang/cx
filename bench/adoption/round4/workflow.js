const evidence = [];
function refs(results) { return results.map(r => ({ ok: r.ok ?? false, runId: r.runId ?? null,
  outputReference: r.outputReference ?? null, artifactPaths: r.artifactPaths ?? [] })); }
for (const phase of ['baseline', 'validation', 'compatibility']) {
  const rows = inventory.filter(r => r.phase === phase || (phase === 'compatibility' && r.phase === 'negative-control'));
  const completed = await runs.all(rows.map(row => ({ key: row.id, label: 'Implement ' + row.case,
    agent: 'edit-trial', model: row.model + ':medium', cwd: row.cwd, task: row.prompt,
    context: 'fresh', skill: false, timeoutMs: 600000, output: row.id + '/result.md',
    acceptance: { level: 'none', reason: 'Independent frozen host evaluation.' } })));
  evidence.push({ phase, runs: refs(completed) });
  if (completed.some(r => !r.ok)) return { blocked: true, phase, evidence };
}
let winner = null;
for (const arm of ['baseline', 'required-entry']) {
  const rows = inventory.filter(r => r.arm === arm);
  const command = 'node ' + JSON.stringify(judgeScript) + ' ' + JSON.stringify(studyRoot) + ' ' +
    JSON.stringify(rows.map(r => r.id).join(',')) + ' ' + (arm === 'baseline' ? 0 : 2);
  const check = await runs.run(arm + '-check', { label: 'Verify source use and behavioral gates', agent: 'trial-judge',
    model: 'amazon-bedrock/global.openai.gpt-6-astra:low', cwd: studyRoot, context: 'fresh', skill: false,
    task: 'Run this exact command once and return the printed JSON without interpretation. Do not edit trials.\n' + command,
    timeoutMs: 600000, output: arm + '-check/result.json',
    outputSchema: { type: 'object', properties: { pass: { type: 'boolean' }, infrastructure: { type: 'boolean' },
      valid: { type: 'integer' }, meaningful: { type: 'integer' }, passed: { type: 'integer' }, total: { type: 'integer' }, report: { type: 'string' } },
      required: ['pass','infrastructure','valid','meaningful','passed','total','report'], additionalProperties: false } });
  evidence.push({ phase: arm + '-check', runs: refs([check]), verdict: check.structuredOutput ?? null });
  emit({ arm, verdict: check.structuredOutput ?? null });
  if (!check.ok || !check.structuredOutput || check.structuredOutput.infrastructure) return { blocked: true, phase: arm + '-check', evidence };
  if (arm === 'required-entry' && check.structuredOutput.pass) winner = arm;
}
return { blocked: false, winner, evidence, note: 'Parent must review preceding shell commands and scope before promotion.' };
