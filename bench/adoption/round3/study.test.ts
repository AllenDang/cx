import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { appendNavigationTail } from './observer.js';

test('system tail preserves the complete preceding prompt and requires active tools', () => {
  assert.equal(appendNavigationTail('original', 'guidance', ['cx_symbols']), 'original\n\nguidance');
  assert.equal(appendNavigationTail('original', 'guidance', ['read']), 'original');
  assert.equal(appendNavigationTail('original', '', ['cx_symbols']), 'original');
});

test('candidates preserve fallbacks and exclude documentation/configuration from navigation policy', () => {
  const tails = JSON.parse(readFileSync(new URL('./tails.json', import.meta.url), 'utf8'));
  for (const entry of Object.values(tails) as any[]) {
    assert.match(entry.text, /grep\/find\/read/);
    assert.match(entry.text, /documentation|文档/);
    assert.match(entry.text, /configuration|配置/);
  }
});
