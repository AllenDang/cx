const evidence = [];
function references(results) {
  return results.map(r => ({ ok: r.ok ?? false, runId: r.runId ?? null,
    outputReference: r.outputReference ?? null, artifactPaths: r.artifactPaths ?? [] }));
}
function editItems(rows) {
  return rows.map(row => ({ key: row.id, label: 'Implement ' + row.case,
    agent: 'edit-trial', model: row.model + ':medium', cwd: row.cwd, task: row.prompt,
    context: 'fresh', skill: false, timeoutMs: 600000, output: row.id + '/result.md',
    acceptance: { level: 'none', reason: 'Independent host evaluation after settlement.' } }));
}
function judgment(key, rows, required) {
  const command = 'node ' + JSON.stringify(judgeScript) + ' ' + JSON.stringify(studyRoot) + ' ' +
    JSON.stringify(rows.map(r => r.id).join(',')) + ' ' + required;
  return runs.run(key, { label: 'Check source retrieval and code tests', agent: 'trial-judge',
    model: 'amazon-bedrock/global.openai.gpt-6-astra:low', cwd: studyRoot, context: 'fresh', skill: false,
    task: 'Run this exact command once and return its stdout JSON without interpretation. Do not edit any trial.\n' + command,
    timeoutMs: 600000, output: key + '/result.json',
    outputSchema: { type: 'object', properties: { pass: { type: 'boolean' }, infrastructure: { type: 'boolean' },
      valid: { type: 'integer' }, meaningful: { type: 'integer' }, passed: { type: 'integer' }, total: { type: 'integer' }, report: { type: 'string' } },
      required: ['pass', 'infrastructure', 'valid', 'meaningful', 'passed', 'total', 'report'], additionalProperties: false } });
}
const controls = inventory.filter(r => r.phase === 'baseline');
const controlRuns = await runs.all(editItems(controls));
evidence.push({ stage: 'baseline', runs: references(controlRuns) });
if (controlRuns.some(r => !r.ok)) return { blocked: true, reason: 'baseline child failure', evidence };
const controlCheck = await judgment('baseline-check', controls, 0);
evidence.push({ stage: 'baseline-check', runs: references([controlCheck]), verdict: controlCheck.structuredOutput ?? null });
if (!controlCheck.ok || !controlCheck.structuredOutput || controlCheck.structuredOutput.infrastructure) return { blocked: true, reason: 'baseline evidence failure', evidence };
for (const arm of ['examples', 'tail-priority']) {
  const development = inventory.filter(r => r.arm === arm && r.phase === 'development');
  const devRuns = await runs.all(editItems(development));
  evidence.push({ stage: arm + '-development', runs: references(devRuns) });
  if (devRuns.some(r => !r.ok)) return { blocked: true, reason: 'development child failure', evidence };
  const devCheck = await judgment(arm + '-development-check', development, 2);
  const devVerdict = devCheck.structuredOutput;
  evidence.push({ stage: arm + '-development-check', runs: references([devCheck]), verdict: devVerdict ?? null });
  emit({ arm, phase: 'development', verdict: devVerdict ?? null });
  if (!devCheck.ok || !devVerdict || devVerdict.infrastructure) return { blocked: true, reason: 'development evidence failure', evidence };
  if (!devVerdict.pass) continue;
  const validation = inventory.filter(r => r.arm === arm && ['validation', 'compatibility', 'negative-control'].includes(r.phase));
  // DeepSeek held-out tasks first, then GPT/Kimi compatibility tasks. Independent writers.
  const testRuns = await runs.all(editItems(validation));
  evidence.push({ stage: arm + '-validation', runs: references(testRuns) });
  if (testRuns.some(r => !r.ok)) return { blocked: true, reason: 'validation child failure', evidence };
  const testCheck = await judgment(arm + '-validation-check', validation, 2);
  const testVerdict = testCheck.structuredOutput;
  evidence.push({ stage: arm + '-validation-check', runs: references([testCheck]), verdict: testVerdict ?? null });
  emit({ arm, phase: 'validation', verdict: testVerdict ?? null });
  if (!testCheck.ok || !testVerdict || testVerdict.infrastructure) return { blocked: true, reason: 'validation evidence failure', evidence };
  if (testVerdict.pass) return { blocked: false, winner: arm, evidence };
}
return { blocked: false, winner: null, reason: 'No preset passed the predeclared threshold; do not claim success.', evidence };
