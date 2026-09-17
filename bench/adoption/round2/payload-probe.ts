import { readFileSync, writeFileSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { observe } from './observer.js';

/** Diagnostic only: writes bounded request metadata, never credentials or raw message content. */
export function probe(pi: ExtensionAPI, root: string): void {
  observe(pi, root);
  const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
  let request = 0;
  pi.on('before_provider_request', (event, ctx) => {
    const record = manifest.records.find((r: any) => r.cwd === realpathSync(ctx.cwd));
    const payload = event.payload as any;
    const serialized = JSON.stringify(payload);
    const names = new Set<string>();
    const roles: string[] = [];
    function visit(value: any): void {
      if (!value || typeof value !== 'object') return;
      if (typeof value.name === 'string' && (value.parameters || value.input_schema)) names.add(value.name);
      if (typeof value.role === 'string') roles.push(value.role);
      for (const child of Object.values(value)) {
        if (Array.isArray(child)) child.forEach(visit);
        else if (child && typeof child === 'object') visit(child);
      }
    }
    visit(payload);
    const checks = record.metadata.map((tool: any) => ({ name: tool.name,
      descriptionPresent: serialized.includes(JSON.stringify(tool.description).slice(1, -1)),
      guidelinesPresent: tool.promptGuidelines.every((text: string) => serialized.includes(JSON.stringify(text).slice(1, -1))),
    }));
    writeFileSync(join(record.evidence, `request-${++request}.json`), JSON.stringify({
      model: typeof payload.model === 'string' ? payload.model : null,
      topLevelKeys: Object.keys(payload), toolNames: [...names], roles, checks,
      taskPresent: serialized.includes(JSON.stringify(record.prompt).slice(1, -1)),
      tailPresent: record.tail ? serialized.includes(JSON.stringify(record.tail).slice(1, -1)) : null,
      toolChoice: typeof payload.tool_choice === 'string' ? payload.tool_choice : null,
    }, null, 2));
  });
}
