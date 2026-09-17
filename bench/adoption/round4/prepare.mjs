import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { registerCxTools } from '../../../extensions/pi-cx/tools.ts';

const root = resolve(process.argv[2] ?? '/tmp/cx-adoption-round4');
if (existsSync(join(root, 'manifest.json'))) throw new Error('Experiment already exists');
const oldRoot = '/tmp/cx-adoption-round3';
const old = JSON.parse(readFileSync(join(oldRoot, 'manifest.json')));
const development = JSON.parse(readFileSync('/tmp/cx-adoption-round2/manifest.json')).records.find(r => r.arm === 'priority' && r.case === 'negative-size');
const policy = JSON.parse(readFileSync(new URL('./policy.json', import.meta.url)));
const records = [];
function add(template, arm, phase, model = template.model) {
  const id = 's' + String(records.length + 1).padStart(2, '0');
  const cwd = join(root, 'trials', id), evidence = join(root, 'evidence', id);
  mkdirSync(join(root, 'trials'), { recursive: true }); mkdirSync(evidence, { recursive: true });
  if (execFileSync(old.git, ['rev-parse', 'HEAD'], { cwd: template.upstream, encoding: 'utf8' }).trim() !== template.commit) throw new Error('Wrong upstream');
  if (execFileSync(old.git, ['status', '--porcelain'], { cwd: template.upstream, encoding: 'utf8' }).trim()) throw new Error('Dirty upstream');
  execFileSync(old.git, ['clone', '--quiet', '--no-hardlinks', template.upstream, cwd]);
  let prompt = template.prompt;
  if (template.case === 'reset-stats') prompt += '\n边界条件说明：即便 cache=None 且 lock 非空，cache_reset_stats() 重置计数时也必须获取该锁。';
  if (/\bcx\b|cx_/i.test(prompt)) throw new Error('Tool name in edit request');
  records.push({ ...template, id, model, prompt, arm, phase, tail: arm === 'baseline' ? '' : policy.text,
    cwd: realpathSync(cwd), evidence, promptSha256: createHash('sha256').update(prompt).digest('hex') });
}
for (const r of old.records.filter(r => r.phase === 'baseline')) add(r, 'baseline', 'baseline');
for (const r of old.records.filter(r => r.arm === 'tail-priority' && r.phase === 'validation')) add(r, 'required-entry', 'validation');
for (const model of ['amazon-bedrock/global.openai.gpt-6-astra', 'aliyun/kimi-k3']) add(development, 'required-entry', 'compatibility', model);
add(old.records.find(r => r.arm === 'tail-priority' && r.phase === 'negative-control'), 'required-entry', 'negative-control');
const tools = []; registerCxTools({ registerTool: tool => tools.push(tool) });
for (const record of records) {
  const warmup = await tools.find(t => t.name === 'cx_overview').execute('host-preflight', {}, undefined, undefined, { cwd: record.cwd, hasUI: false });
  writeFileSync(join(record.evidence, 'preflight.json'), JSON.stringify(warmup));
}
const hashes = {};
for (const file of ['policy.json','lib.mjs','judge.mjs','workflow.js','../../../extensions/pi-cx/tools.ts']) hashes[file] = createHash('sha256').update(readFileSync(new URL(file, import.meta.url))).digest('hex');
writeFileSync(join(root, 'manifest.json'), JSON.stringify({ ...old, version: 4, createdAt: new Date().toISOString(), policy, hashes, records }, null, 2));
const observer = fileURLToPath(new URL('../round3/observer.ts', import.meta.url));
writeFileSync(join(root, 'extension.ts'), `import { observeTail } from ${JSON.stringify(observer)};\nexport default function(pi:any){observeTail(pi,${JSON.stringify(root)});}\n`);
mkdirSync(join(root, '.pi/agents'), { recursive: true });
writeFileSync(join(root, '.pi/agents/edit-trial.md'), readFileSync(join(oldRoot, '.pi/agents/edit-trial.md'),'utf8').replace(join(oldRoot,'extension.ts'),join(root,'extension.ts')));
writeFileSync(join(root, '.pi/agents/trial-judge.md'), readFileSync(join(oldRoot, '.pi/agents/trial-judge.md')));
const inventory = records.map(({ metadata, evidence, tail, ...rest }) => rest);
const judgeScript = fileURLToPath(new URL('./judge.mjs', import.meta.url));
writeFileSync(join(root, 'run.js'), `const inventory = ${JSON.stringify(inventory)};\nconst studyRoot = ${JSON.stringify(root)};\nconst judgeScript = ${JSON.stringify(judgeScript)};\n` + readFileSync(new URL('./workflow.js', import.meta.url),'utf8'));
console.log(JSON.stringify({ root, prepared: records.length, hashes }));
