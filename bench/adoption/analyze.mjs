import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const exploration = new Set(["read", "grep", "find", "ls"]);
export function isNavigation(call) {
  return call.name.startsWith("cx_") || exploration.has(call.name) ||
    (call.name === "bash" && /(?:^|[\s;|&])(rg|grep|find|ls|cat|sed|head|tail|awk)\s/.test(call.args?.command ?? ""));
}
export function summarize(events) {
  const calls = events.filter(e => e.type === "tool_start");
  const ends = events.filter(e => e.type === "tool_end");
  const navigation = calls.filter(isNavigation);
  const cx = calls.filter(e => e.name.startsWith("cx_"));
  const byTool = {};
  for (const call of calls) byTool[call.name] = (byTool[call.name] ?? 0) + 1;
  const usage = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 };
  for (const event of events.filter(e => e.type === "assistant_end")) {
    for (const field of ["input", "output", "cacheRead", "cacheWrite"]) usage[field] += event.usage?.[field] ?? 0;
    usage.cost += event.usage?.cost?.total ?? 0;
  }
  return {
    calls: calls.length, cxCalls: cx.length, navigationCalls: navigation.length,
    cxShareAll: calls.length ? cx.length / calls.length : null,
    cxShareNavigation: navigation.length ? cx.length / navigation.length : null,
    adopted: cx.length > 0, firstNavigation: navigation[0]?.name ?? null,
    firstCxPosition: cx.length ? calls.indexOf(cx[0]) + 1 : null,
    toolErrors: ends.filter(e => e.isError).length,
    cxErrors: ends.filter(e => e.name.startsWith("cx_") && e.isError).length,
    cxEmpty: ends.filter(e => e.name.startsWith("cx_") && !e.isError && e.result?.details?.resultCount === 0).length,
    byTool, usage, elapsedMs: events.filter(e => e.type === "end").at(-1)?.elapsedMs ?? null,
  };
}
export function checkExposure(record, exposure, baseline, candidate) {
  if (!exposure) return "missing exposure";
  if (exposure.model !== record.model) return "model mismatch";
  // pi-subagents adds a fixed Task prefix and authoritative output binding.
  // Accept only that exact wrapper, not arbitrary instructions around the task.
  const wrapper = exposure.prompt.match(/^Task: ([\s\S]*)\n\n---\n\*\*Output:\*\*\nWrite your findings to exactly this path: ([^\n]+)\nThis path is authoritative for this run\.\nIgnore any other output filename or output path mentioned elsewhere, including output destinations in the base agent prompt, system prompt, or task instructions\.$/);
  const task = wrapper ? wrapper[1] : exposure.prompt;
  if (task !== record.prompt) return "task prompt mismatch";
  if (wrapper && !wrapper[2].endsWith(`/${record.id}/result.md`)) return "output binding mismatch";
  for (const tool of baseline) {
    if (!exposure.active.includes(tool.name)) return `inactive ${tool.name}`;
    const actual = exposure.tools.find(t => t.name === tool.name);
    const expected = record.variant === "candidate" ? candidate[tool.name] : tool.description;
    if (actual?.description !== expected) return `description mismatch ${tool.name}`;
    if (JSON.stringify(actual?.parameters) !== JSON.stringify(tool.parameters)) return `schema mismatch ${tool.name}`;
    if (JSON.stringify(actual?.promptGuidelines) !== JSON.stringify(tool.promptGuidelines)) return `guideline mismatch ${tool.name}`;
  }
  if (/\bcx\b|cx_/i.test(record.prompt)) return "task leaks tool name";
  return null;
}

export function pairedRows(rows) {
  return rows.filter(row => row.valid && rows.some(other =>
    other.valid && other.model === row.model && other.case === row.case && other.variant !== row.variant));
}

function json(path) { return JSON.parse(readFileSync(path, "utf8")); }
export function analyze(root) {
  const manifest = json(join(root, "manifest.json"));
  const baseline = json(join(root, "baseline.json"));
  const candidate = json(join(root, "candidate.json"));
  const rows = manifest.records.map(record => {
    const file = join(record.evidence, "events.jsonl");
    if (!existsSync(file)) return { id: record.id, model: record.model, case: record.case, variant: record.variant, valid: false, reason: "not observed" };
    const events = readFileSync(file, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
    const exposure = json(join(record.evidence, "exposure.json"));
    const starts = events.filter(e => e.type === "start");
    const providerError = events.find(e => e.type === "assistant_end" && ["error", "aborted"].includes(e.stopReason));
    const reason = checkExposure(record, exposure, baseline, candidate) ??
      (starts.length !== 1 ? "multiple attempts/turns" : null) ??
      (providerError ? `provider/runtime error: ${providerError.errorMessage ?? providerError.stopReason}` : null) ??
      (!events.some(e => e.type === "end") ? "not settled" : null);
    const row = { id: record.id, model: record.model, case: record.case, variant: record.variant, valid: !reason, reason, ...summarize(events) };
    if (!events.some(e => e.type === "end")) return row;
    const env = { ...process.env, PYTHONPATH: join(record.cwd, "src"), PYTHONDONTWRITEBYTECODE: "1" };
    const commands = [
      ["regression", ["-m", "pytest", "-q"]],
      ["frozenRegression", ["-m", "pytest", "-q", join(root, "upstream", "tests")]],
      ["behavior", [resolve(new URL("./acceptance.py", import.meta.url).pathname), record.case]],
    ];
    for (const [label, args] of commands) {
      const result = spawnSync("python3", args, { cwd: record.cwd, env, encoding: "utf8", timeout: 120000 });
      writeFileSync(join(record.evidence, `${label}.log`), `${result.stdout ?? ""}${result.stderr ?? ""}${result.error ?? ""}`);
      row[label] = result.status === 0 && !result.error;
    }
    const diff = spawnSync("git", ["diff", "--binary", "HEAD"], { cwd: record.cwd, encoding: "utf8" });
    if (diff.error || diff.status !== 0) throw new Error(`Cannot capture diff for ${record.id}: ${diff.error ?? diff.stderr}`);
    writeFileSync(join(record.evidence, "tracked.patch"), diff.stdout ?? "");
    const status = spawnSync("git", ["status", "--short"], { cwd: record.cwd, encoding: "utf8" });
    if (status.error || status.status !== 0) throw new Error(`Cannot capture status for ${record.id}: ${status.error ?? status.stderr}`);
    writeFileSync(join(record.evidence, "git-status.txt"), status.stdout ?? "");
    row.passed = row.regression && row.frozenRegression && row.behavior;
    return row;
  });
  const groups = [];
  for (const model of [...new Set(rows.map(r => r.model))]) for (const variant of ["baseline", "candidate"]) {
    const planned = rows.filter(r => r.model === model && r.variant === variant);
    const valid = planned.filter(r => r.valid);
    const sum = field => valid.reduce((n, r) => n + r[field], 0);
    groups.push({ model, variant, planned: planned.length, valid: valid.length,
      adopted: valid.filter(r => r.adopted).length, passed: valid.filter(r => r.passed).length,
      cxCalls: sum("cxCalls"), calls: sum("calls"), navigationCalls: sum("navigationCalls"),
      cxErrors: sum("cxErrors"), toolErrors: sum("toolErrors"), elapsedMs: sum("elapsedMs") });
  }
  const report = { version: 2, acceptanceVersion: 2, analyzedAt: new Date().toISOString(), rows, groups,
    pairedIds: pairedRows(rows).map(row => row.id) };
  writeFileSync(join(root, "summary.json"), JSON.stringify(report, null, 2));
  return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(JSON.stringify(analyze(resolve(process.argv[2])).groups, null, 2));
}
