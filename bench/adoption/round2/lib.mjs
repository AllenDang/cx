import { summarize } from '../analyze.mjs';

export function resolveMetadata(baseline, presets, name) {
  if (name === 'baseline') return structuredClone(baseline);
  const preset = presets[name];
  if (!preset) throw new Error(`Unknown preset: ${name}`);
  const parent = resolveMetadata(baseline, presets, preset.extends ?? 'baseline');
  return parent.map(tool => ({ ...tool, ...preset.tools[tool.name] }));
}

export function inspectTrial(record, exposure, events) {
  const issues = [];
  if (exposure.model !== record.model) issues.push('wrong model');
  if (!exposure.prompt.startsWith(`Task: ${record.prompt}\n\n---\n**Output:**\n`)) issues.push('task drift');
  if (events.filter(e => e.type === 'start').length !== 1) issues.push('multiple attempts');
  if (!events.some(e => e.type === 'end')) issues.push('not ended');
  if (events.some(e => e.type === 'assistant_end' && ['error', 'aborted'].includes(e.stopReason))) issues.push('provider/runtime error');
  for (const expected of record.metadata) {
    const actual = exposure.tools.find(t => t.name === expected.name);
    if (!exposure.active.includes(expected.name)) issues.push(`inactive ${expected.name}`);
    for (const field of ['description', 'parameters', 'promptGuidelines']) {
      if (JSON.stringify(actual?.[field]) !== JSON.stringify(expected[field])) issues.push(`${expected.name} ${field} drift`);
    }
    if (!exposure.systemPrompt.includes(expected.promptSnippet)) issues.push(`${expected.name} snippet missing`);
    for (const text of expected.promptGuidelines) if (!exposure.systemPrompt.includes(text)) issues.push(`${expected.name} guideline missing`);
  }
  const stats = summarize(events);
  const calls = events.filter(e => e.type === 'tool_start');
  // Source retrieval, not an overview-only invocation or repeated refresh, earns adoption.
  // Shell writes cannot be classified perfectly; all events remain available for review.
  const firstMutation = calls.findIndex(e => ['edit', 'write', 'hashline_edit'].includes(e.name) ||
    (e.name === 'bash' && /(?:write_text|writeFile|sed\s+-i|apply_patch|\btee\b|\bcat\b[^\n]*>)/.test(e.args?.command ?? '')));
  const useful = calls.filter((e, i) => ['cx_symbols', 'cx_definition', 'cx_context'].includes(e.name) &&
    (firstMutation < 0 || i < firstMutation) && events.some(end => end.type === 'tool_end' && end.id === e.id &&
      !end.isError && end.result?.details?.resultCount > 0));
  return { issues, ...stats, usefulSourceCalls: useful.length, meaningfulAdoption: useful.length > 0 };
}
