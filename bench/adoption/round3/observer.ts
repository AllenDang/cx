import { readFileSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { probe } from '../round2/payload-probe.js';

export function appendNavigationTail(prompt: string, tail: string, active: string[]): string {
  return tail && active.includes('cx_symbols') ? prompt + '\n\n' + tail : prompt;
}

export function observeTail(pi: ExtensionAPI, root: string): void {
  const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
  // Register first so the observer sees the final chained prompt, not its predecessor.
  pi.on('before_agent_start', (event, ctx) => {
    const record = manifest.records.find((r: any) => r.cwd === realpathSync(ctx.cwd));
    if (!record) throw new Error('Unknown trial cwd');
    const systemPrompt = appendNavigationTail(event.systemPrompt, record.tail, pi.getActiveTools());
    if (systemPrompt !== event.systemPrompt) return { systemPrompt };
  });
  probe(pi, root);
}
