import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { registerCxTools } from '../../../extensions/pi-cx/tools.ts';
import { resolveMetadata } from './lib.mjs';

const root = resolve(process.argv[2] ?? '/tmp/cx-adoption-round2');
if (existsSync(join(root, 'manifest.json'))) throw new Error('Refusing to overwrite an experiment');
const read = name => JSON.parse(readFileSync(new URL(name, import.meta.url), 'utf8'));
const baseline = read('../baseline.json');
const presets = read('./presets.json');
const heldout = read('./heldout.json');
const old = read('../cases.json');
const git = existsSync('/Library/Developer/CommandLineTools/usr/bin/git') ? '/Library/Developer/CommandLineTools/usr/bin/git' : 'git';
const pathPrefix = `${root}/venv/bin:/opt/homebrew/bin:/Library/Developer/CommandLineTools/usr/bin`;
const upstreams = {
  cachetools: { path: resolve(process.argv[3] ?? '/tmp/cx-adoption-study/upstream'), commit: old.commit },
  blinker: { path: join(root, 'blinker-upstream'), commit: '669f3a027828d19786e708b511277fabcd6b9532' },
};
for (const upstream of Object.values(upstreams)) {
  if (execFileSync(git, ['rev-parse', 'HEAD'], { cwd: upstream.path, encoding: 'utf8' }).trim() !== upstream.commit) throw new Error('Wrong upstream commit');
  if (execFileSync(git, ['status', '--porcelain'], { cwd: upstream.path, encoding: 'utf8' }).trim()) throw new Error('Dirty upstream');
}
const tools = [];
registerCxTools({ registerTool: tool => tools.push(tool) });
const records = [];
const models = old.models;
function add(arm, phase, task, model) {
  const id = `s${String(records.length + 1).padStart(2, '0')}`;
  const cwd = join(root, 'trials', id);
  const evidence = join(root, 'evidence', id);
  mkdirSync(evidence, { recursive: true });
  mkdirSync(join(root, 'trials'), { recursive: true });
  const upstream = upstreams[task.repository ?? 'cachetools'];
  execFileSync(git, ['clone', '--quiet', '--no-hardlinks', upstream.path, cwd]);
  const prompt = `${task.prompt}\n\n${old.suffix}`;
  if (/\bcx\b|cx_/i.test(prompt)) throw new Error('Task leaks tool instructions');
  const metadata = resolveMetadata(baseline, presets, arm);
  records.push({ id, arm, phase, case: task.id, repository: task.repository ?? 'cachetools', upstream: upstream.path,
    commit: upstream.commit, model, cwd: realpathSync(cwd), evidence, prompt, metadata,
    promptSha256: createHash('sha256').update(prompt).digest('hex'),
    acceptance: fileURLToPath(new URL(phase === 'development' ? '../acceptance.py' : './acceptance.py', import.meta.url)) });
}
for (const task of heldout) add('baseline', 'baseline', task, models[2]);
for (const arm of Object.keys(presets)) {
  for (const task of old.cases.filter(t => ['negative-size', 'peek'].includes(t.id))) add(arm, 'development', task, models[2]);
  for (const task of heldout) add(arm, 'validation', task, models[2]);
  for (const model of models.slice(0, 2)) add(arm, 'compatibility', heldout[1], model);
}
for (const record of records) {
  const warmup = await tools.find(t => t.name === 'cx_overview').execute('host-preflight', {}, undefined, undefined, { cwd: record.cwd, hasUI: false });
  writeFileSync(join(record.evidence, 'preflight.json'), JSON.stringify(warmup));
}
const hashes = {};
for (const file of ['presets.json', 'heldout.json', 'acceptance.py', '../acceptance.py']) hashes[file] = createHash('sha256').update(readFileSync(new URL(file, import.meta.url))).digest('hex');
writeFileSync(join(root, 'manifest.json'), JSON.stringify({ version: 2, createdAt: new Date().toISOString(), pathPrefix,
  python: `${root}/venv/bin/python3`, git, presets, hashes, records }, null, 2));
const observer = fileURLToPath(new URL('./observer.ts', import.meta.url));
writeFileSync(join(root, 'extension.ts'), `import { observe } from ${JSON.stringify(observer)};\nexport default function(pi: any) { observe(pi, ${JSON.stringify(root)}); }\n`);
mkdirSync(join(root, '.pi', 'agents'), { recursive: true });
const contract = ['systemPromptMode: append', 'inheritProjectContext: false', 'inheritGlobalContext: false', 'inheritSkills: false', 'thinking: medium'];
writeFileSync(join(root, '.pi/agents/edit-trial.md'), ['---', 'name: edit-trial', 'description: Isolated code editing trial',
  'tools: read, bash, edit, write, grep, find, ls, ' + tools.map(t => t.name).join(', '), 'extensions: ' + join(root, 'extension.ts'), ...contract,
  'acceptance: {"level":"none","reason":"Independent host-side experiment evaluation."}', '---', '',
  'Implement the requested change in the current repository. Preserve existing behavior outside the request, add focused regression tests, and report actual validation results. Do not commit or push. Do not access other workspaces.', ''].join('\n'));
writeFileSync(join(root, '.pi/agents/trial-judge.md'), ['---', 'name: trial-judge', 'description: Read-only deterministic trial evidence relay',
  'tools: bash, read', 'extensions:', 'systemPromptMode: replace', 'inheritProjectContext: false', 'inheritGlobalContext: false', 'inheritSkills: false',
  'thinking: low', 'acceptanceRole: read-only', 'completionGuard: false', '---', '',
  'Run only the exact host evaluation command supplied in your task. Do not change code, repair trials, or reinterpret the evaluator criteria. Return the JSON object printed by that command verbatim through structured_output. If the command fails, report infrastructure=true and pass=false. Do not run any agent.', ''].join('\n'));
const inventory = records.map(({ metadata, evidence, ...rest }) => rest);
const judgeScript = fileURLToPath(new URL('./judge.mjs', import.meta.url));
writeFileSync(join(root, 'run.js'), `const inventory = ${JSON.stringify(inventory)};\nconst studyRoot = ${JSON.stringify(root)};\nconst judgeScript = ${JSON.stringify(judgeScript)};\n` + readFileSync(new URL('./workflow.js', import.meta.url), 'utf8'));
console.log(JSON.stringify({ root, prepared: records.length, arms: Object.keys(presets), hashes }));
