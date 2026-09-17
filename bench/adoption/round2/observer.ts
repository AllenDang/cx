import { appendFileSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { registerCxTools } from '../../../extensions/pi-cx/tools.js';
import { registerDirtyPathProtocol } from '../../../extensions/pi-cx/dirty-paths.js';

export function observe(pi: ExtensionAPI, root: string): void {
  const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
  // Identical dependency/toolchain environment in every arm; no user config changes.
  process.env.PATH = manifest.pathPrefix + ':' + process.env.PATH;
  let record: any;
  let started = 0;
  const log = (data: any) => appendFileSync(join(record.evidence, 'events.jsonl'), JSON.stringify({ at: Date.now(), ...data }) + '\n');
  const tools: any[] = [];
  registerCxTools({ registerTool: (tool: any) => tools.push(tool) } as any, registerDirtyPathProtocol(pi));
  for (const tool of tools) pi.registerTool(tool);
  pi.on('session_start', (_event, ctx) => {
    record = manifest.records.find((r: any) => r.cwd === realpathSync(ctx.cwd));
    if (!record) throw new Error('Unknown trial cwd');
    for (const tool of tools) {
      const metadata = record.metadata.find((t: any) => t.name === tool.name);
      if (JSON.stringify(tool.parameters) !== JSON.stringify(metadata.parameters)) throw new Error('Schema changed');
      pi.registerTool({ ...tool, ...metadata });
    }
  });
  pi.on('before_agent_start', (event, ctx) => {
    started = Date.now();
    writeFileSync(join(record.evidence, 'exposure.json'), JSON.stringify({ active: pi.getActiveTools(), tools: pi.getAllTools(),
      systemPrompt: event.systemPrompt, model: `${ctx.model?.provider}/${ctx.model?.id}`, thinking: ctx.thinkingLevel, prompt: event.prompt }, null, 2));
    log({ type: 'start' });
  });
  pi.on('tool_execution_start', e => log({ type: 'tool_start', id: e.toolCallId, name: e.toolName, args: e.args }));
  pi.on('tool_execution_end', e => log({ type: 'tool_end', id: e.toolCallId, name: e.toolName, isError: e.isError, result: e.result }));
  pi.on('message_end', e => {
    if (e.message.role === 'assistant') {
      const m = e.message as any;
      log({ type: 'assistant_end', usage: m.usage, stopReason: m.stopReason, errorMessage: m.errorMessage });
    }
  });
  pi.on('agent_end', () => log({ type: 'end', elapsedMs: Date.now() - started }));
}
