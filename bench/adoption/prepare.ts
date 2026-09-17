import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, existsSync, realpathSync } from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { registerCxTools } from "../../extensions/pi-cx/tools.js";

const root = resolve(process.argv[2] ?? "/tmp/cx-adoption-study");
const upstream = join(root, "upstream");
const spec = JSON.parse(readFileSync(new URL("./cases.json", import.meta.url), "utf8"));
const candidate = JSON.parse(readFileSync(new URL("./candidate.json", import.meta.url), "utf8"));
const tools: any[] = [];
registerCxTools({ registerTool: (tool: any) => tools.push(tool) } as any);
const baseline = JSON.parse(readFileSync(new URL("./baseline.json", import.meta.url), "utf8"));
for (const tool of tools) {
  const frozen = baseline.find((t: any) => t.name === tool.name);
  if (!frozen || JSON.stringify(tool.parameters) !== JSON.stringify(frozen.parameters)) throw new Error("Frozen baseline schema drift");
}
if (Object.keys(candidate).sort().join() !== tools.map(t => t.name).sort().join()) throw new Error("Candidate tool set mismatch");
if (existsSync(join(root, "manifest.json"))) throw new Error("Refusing to overwrite an existing experiment");
const commit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: upstream, encoding: "utf8" }).trim();
if (commit !== spec.commit) throw new Error("Upstream commit mismatch");
const records: any[] = [];
// Each wave contains all three models. Counterbalance AB/BA by task/model.
for (let taskIndex = 0; taskIndex < spec.cases.length; taskIndex++) {
  for (let period = 0; period < 2; period++) {
    for (let modelIndex = 0; modelIndex < spec.models.length; modelIndex++) {
      const task = spec.cases[taskIndex];
      const variant = (taskIndex + modelIndex + period) % 2 ? "candidate" : "baseline";
      const id = `r${String(records.length + 1).padStart(2, "0")}`;
      const cwd = join(root, "trials", id);
      mkdirSync(join(root, "trials"), { recursive: true });
      execFileSync("git", ["clone", "--quiet", "--no-hardlinks", upstream, cwd]);
      const evidence = join(root, "evidence", id);
      mkdirSync(evidence, { recursive: true });
      const prompt = `${task.prompt}\n\n${spec.suffix}`;
      if (/\bcx\b|cx_/i.test(prompt)) throw new Error("Tool-name leakage in task prompt");
      const record = { id, case: task.id, model: spec.models[modelIndex], variant, cwd: realpathSync(cwd), evidence, prompt,
        promptSha256: createHash("sha256").update(prompt).digest("hex") };
      records.push(record);
      // Host-only warmup, never included in model usage metrics or context.
      const warmup = await tools.find(t => t.name === "cx_overview").execute("preflight", {}, undefined, undefined, { cwd, hasUI: false });
      writeFileSync(join(evidence, "preflight.json"), JSON.stringify(warmup));
    }
  }
}
writeFileSync(join(root, "baseline.json"), JSON.stringify(baseline, null, 2));
writeFileSync(join(root, "candidate.json"), JSON.stringify(candidate, null, 2));
writeFileSync(join(root, "manifest.json"), JSON.stringify({ version: 1, createdAt: new Date().toISOString(), commit, records }, null, 2));
const observer = fileURLToPath(new URL("./observer.ts", import.meta.url));
writeFileSync(join(root, "extension.ts"), `import { studyExtension } from ${JSON.stringify(observer)};\nexport default function(pi: any) { studyExtension(pi, ${JSON.stringify(root)}); }\n`);
mkdirSync(join(root, ".pi", "agents"), { recursive: true });
writeFileSync(join(root, ".pi", "agents", "edit-trial.md"), [
  "---", "name: edit-trial", "description: Isolated code editing trial",
  "tools: read, bash, edit, write, grep, find, ls, " + tools.map(t => t.name).join(", "),
  "extensions: " + join(root, "extension.ts"),
  "systemPromptMode: append", "inheritProjectContext: false", "inheritGlobalContext: false",
  "inheritSkills: false", "thinking: medium",
  'acceptance: {"level":"none","reason":"Host evaluates each isolated trial with frozen tests after completion."}',
  "---", "",
  "Implement the requested change in the current repository. Preserve existing behavior outside the request, add focused regression tests, and report actual validation results. Do not commit or push. Do not access other workspaces.", "",
].join("\n"));
const inventory = records.map(({ id, case: task, model, cwd, prompt }) => ({ id, case: task, model, cwd, prompt }));
writeFileSync(join(root, "run.js"), "const inventory = " + JSON.stringify(inventory) + ";\n" +
  readFileSync(new URL("./workflow.js", import.meta.url), "utf8").replaceAll("args.records", "inventory"));
console.log(JSON.stringify({ root, trials: records.length, tools: tools.map(t => t.name) }));
