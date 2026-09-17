import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { registerCxTools } from '../../../extensions/pi-cx/tools.ts';

const root = resolve(process.argv[2] ?? '/tmp/cx-adoption-round3');
const priorRoot = resolve(process.argv[3] ?? '/tmp/cx-adoption-round2');
if (existsSync(join(root, 'manifest.json'))) throw new Error('Refusing to overwrite experiment');
const old = JSON.parse(readFileSync(join(priorRoot, 'manifest.json')));
const tails = JSON.parse(readFileSync(new URL('./tails.json', import.meta.url)));
const records = [];
function add(template, arm, phase, tail) {
  const id = 's' + String(records.length + 1).padStart(2, '0');
  const cwd = join(root, 'trials', id);
  const evidence = join(root, 'evidence', id);
  mkdirSync(join(root, 'trials'), { recursive: true });
  mkdirSync(evidence, { recursive: true });
  if (execFileSync(old.git, ['rev-parse', 'HEAD'], { cwd: template.upstream, encoding: 'utf8' }).trim() !== template.commit) throw new Error('Wrong upstream');
  if (execFileSync(old.git, ['status', '--porcelain'], { cwd: template.upstream, encoding: 'utf8' }).trim()) throw new Error('Dirty upstream');
  execFileSync(old.git, ['clone', '--quiet', '--no-hardlinks', template.upstream, cwd]);
  if (/\bcx\b|cx_/i.test(template.prompt)) throw new Error('Tool leakage in task');
  records.push({ ...template, id, arm, phase, tail, cwd: realpathSync(cwd), evidence,
    promptSha256: createHash('sha256').update(template.prompt).digest('hex') });
}
for (const template of old.records.filter(r => r.phase === 'baseline')) add(template, 'baseline', 'baseline', '');
for (const [arm, config] of Object.entries(tails)) {
  const templates = old.records.filter(r => r.arm === config.metadataPreset);
  for (const template of templates) add(template, arm, template.phase, config.text);
  const docs = { ...templates[0], case: 'docs-note',
    prompt: '在 README.rst 文末追加一个独立段落，文本为：Contributor note: documentation-only changes do not alter cache behavior.\n只修改该文档，不修改代码或测试；保留已有内容，确认新段落仅出现一次。不提交、不推送，也不要读取其他工作目录。完成后简要报告修改与检查结果。',
    acceptance: fileURLToPath(new URL('./docs_acceptance.py', import.meta.url)) };
  add(docs, arm, 'negative-control', config.text);
}
const tools = [];
registerCxTools({ registerTool: tool => tools.push(tool) });
for (const record of records) {
  const result = await tools.find(t => t.name === 'cx_overview').execute('host-preflight', {}, undefined, undefined, { cwd: record.cwd, hasUI: false });
  writeFileSync(join(record.evidence, 'preflight.json'), JSON.stringify(result));
}
const hashes = {};
for (const file of ['tails.json', 'observer.ts', 'judge.mjs', 'docs_acceptance.py', 'workflow.js']) hashes[file] = createHash('sha256').update(readFileSync(new URL(file, import.meta.url))).digest('hex');
writeFileSync(join(root, 'manifest.json'), JSON.stringify({ ...old, version: 3, createdAt: new Date().toISOString(), hashes, tails, records }, null, 2));
const observer = fileURLToPath(new URL('./observer.ts', import.meta.url));
writeFileSync(join(root, 'extension.ts'), `import { observeTail } from ${JSON.stringify(observer)};\nexport default function(pi:any) { observeTail(pi, ${JSON.stringify(root)}); }\n`);
mkdirSync(join(root, '.pi/agents'), { recursive: true });
const editAgent = readFileSync(join(priorRoot, '.pi/agents/edit-trial.md'), 'utf8').replace(join(priorRoot, 'extension.ts'), join(root, 'extension.ts'));
writeFileSync(join(root, '.pi/agents/edit-trial.md'), editAgent);
writeFileSync(join(root, '.pi/agents/trial-judge.md'), readFileSync(join(priorRoot, '.pi/agents/trial-judge.md')));
const inventory = records.map(({ metadata, evidence, tail, ...rest }) => rest);
const judgeScript = fileURLToPath(new URL('./judge.mjs', import.meta.url));
writeFileSync(join(root, 'run.js'), `const inventory = ${JSON.stringify(inventory)};\nconst studyRoot = ${JSON.stringify(root)};\nconst judgeScript = ${JSON.stringify(judgeScript)};\n` + readFileSync(new URL('./workflow.js', import.meta.url), 'utf8'));
console.log(JSON.stringify({ root, prepared: records.length, hashes }));
