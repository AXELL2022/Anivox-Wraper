import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = file => readFileSync(new URL(`../ui/${file}`, import.meta.url), 'utf8');
const nav = html => {
  const match = /<nav\b[\s\S]*?<\/nav>/.exec(html);
  assert.ok(match, 'toolbar <nav> not found');
  return match[0].replace(/\r\n/g, '\n').split('\n').map(line => line.trim()).filter(Boolean);
};

// Windows loads ui/index.html as its own webview; Linux injects ui/toolbar-fragment.html into the site.
// CLAUDE.md requires every toolbar button to exist in both, so the markup must stay identical.
test('Windows toolbar and Linux toolbar fragment share the same markup', () => {
  assert.deepEqual(nav(read('toolbar-fragment.html')), nav(read('index.html')));
});
