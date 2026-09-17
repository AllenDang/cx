import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { inspectTrial } from '../round2/lib.mjs';

const root = resolve(process.argv[2]);
const ids = process.argv[3].split(',');
const required = Number(process.argv[4]);
if (!ids.length || ids.some(id => !/^s\d{2}$/.test(id)) || !Number.isInteger(required) || required < 0) throw new Error('Invalid evaluation request');
const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
const rows = [];
for (const id of ids) {
  const record = manifest.records.find(r => r.id === id);
  if (!record) throw new Error('Unknown trial ' + id);
  const exposurePath = join(record.evidence, 'exposure.json');
  const eventsPath = join(record.evidence, 'events.jsonl');
  if (!existsSync(exposurePath) || !existsSync(eventsPath)) {
    rows.push({ id, issues: ['missing evidence'], passed: false, meaningfulAdoption: false });
    continue;
  }
  const exposure = JSON.parse(readFileSync(exposurePath));
  const events = readFileSync(eventsPath, 'utf8').trim().split('\n').map(JSON.parse);
  const stats = inspectTrial(record, exposure, events);
  const requestPath = join(record.evidence, 'request-1.json');
  if (!existsSync(requestPath)) stats.issues.push('missing request telemetry');
  else {
    const request = JSON.parse(readFileSync(requestPath));
    if (!request.taskPresent || request.checks.some(t => !t.descriptionPresent || !t.guidelinesPresent)) stats.issues.push('request metadata drift');
    if (record.tail && (!request.tailPresent || !exposure.systemPrompt.endsWith(record.tail))) stats.issues.push('system-tail drift');
  }
  const row = { id, model: record.model, arm: record.arm, phase: record.phase, case: record.case, ...stats };
  const env = { ...process.env, PATH: manifest.pathPrefix + ':' + process.env.PATH,
    PYTHONPATH: join(record.cwd, 'src'), PYTHONDONTWRITEBYTECODE: '1' };
  if (!events.some(e => e.type === 'end')) { rows.push({ ...row, passed: false }); continue; }
  const commands = [
    ['regression', ['-m', 'pytest', '-q']],
    ['frozenRegression', ['-m', 'pytest', '-q', join(record.upstream, 'tests')]],
    ['behavior', [record.acceptance, record.case]],
  ];
  for (const [label, args] of commands) {
    const result = spawnSync(manifest.python, args, { cwd: record.cwd, env, encoding: 'utf8', timeout: 120000 });
    writeFileSync(join(record.evidence, label + '.log'), `${result.stdout ?? ''}${result.stderr ?? ''}${result.error ?? ''}`);
    row[label] = result.status === 0 && !result.error;
    if (result.error) row.issues.push(label + ': ' + result.error.message);
  }
  for (const [name, args] of [['tracked.patch', ['diff', '--binary', 'HEAD']], ['git-status.txt', ['status', '--short']]]) {
    const result = spawnSync(manifest.git, args, { cwd: record.cwd, env, encoding: 'utf8', timeout: 30000 });
    if (result.error || result.status !== 0) throw new Error(`Git evidence failed ${id}: ${result.error ?? result.stderr}`);
    writeFileSync(join(record.evidence, name), result.stdout);
  }
  row.passed = row.regression && row.frozenRegression && row.behavior;
  rows.push(row);
}
const valid = rows.filter(r => !r.issues.length);
const target = valid.filter(r => r.model === 'aliyun/deepseek-v4.1-flash' && r.phase !== 'negative-control');
const meaningful = target.filter(r => r.meaningfulAdoption).length;
const infrastructure = valid.length !== rows.length;
const passed = rows.filter(r => r.passed).length;
const negativeControlsClean = rows.filter(r => r.phase === 'negative-control').every(r => r.cxCalls === 0);
const report = join(root, 'judgments', ids.join('-') + '.json');
mkdirSync(join(root, 'judgments'), { recursive: true });
const verdict = { pass: !infrastructure && passed === rows.length && meaningful >= required && negativeControlsClean,
  infrastructure, valid: valid.length, meaningful, passed, total: rows.length, report };
writeFileSync(report, JSON.stringify({ verdict, rows }, null, 2));
console.log(JSON.stringify(verdict));
