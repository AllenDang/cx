import { resolve, sep } from 'node:path';
import { inspectTrial as inspectBase } from '../round2/lib.mjs';

export function possibleShellMutation(command) {
  // stderr redirection (2>/dev/null) is not a source-file write.
  return /\b(?:apply_patch|tee)\b|\bsed\s+[^\n]*-i\b|\b(?:write_text|write_bytes|writeFile)\s*\(|\bopen\([^\n]*,[ \t]*["'][wa]/.test(command) ||
    /\bcat\b[^\n]*\s(?<![0-9])>{1,2}\s*(?!\/dev\/null)[^\s]+/.test(command);
}

export function inspectTrial(record, exposure, events) {
  const stats = inspectBase(record, exposure, events);
  const calls = events.filter(e => e.type === 'tool_start');
  const mutation = calls.find(e => {
    if (!['edit', 'write'].includes(e.name) || typeof e.args?.path !== 'string') return false;
    const path = resolve(record.cwd, e.args.path.replace(/^@/, ''));
    return path.startsWith(record.cwd + sep);
  });
  const useful = calls.filter(e => ['cx_symbols', 'cx_definition', 'cx_context'].includes(e.name) &&
    events.some(end => end.type === 'tool_end' && end.id === e.id && !end.isError &&
      end.result?.details?.resultCount > 0 && (!mutation || end.at < mutation.at)));
  const priorShellCandidates = calls.filter(e => e.name === 'bash' && (!mutation || e.at < mutation.at) &&
    possibleShellMutation(e.args?.command ?? ''));
  return { ...stats, usefulSourceCalls: useful.length, meaningfulAdoption: useful.length > 0,
    shellMutationCandidates: priorShellCandidates.map(e => e.id), requiresManualMutationAudit: priorShellCandidates.length > 0,
    firstDirectMutationAt: mutation?.at ?? null };
}
