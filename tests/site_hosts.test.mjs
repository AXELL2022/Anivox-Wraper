import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const hosts = [...read('src-tauri/anivox-core/src/site_hosts.rs').match(/SITE_HOSTS: \[&str; \d+\] = \[([^\]]*)\]/)[1]
  .matchAll(/"([^"]+)"/g)].map(match => match[1]);

test('site host list has the main site and at least one mirror', () => {
  assert.equal(hosts[0], 'anivox.fun');
  assert.ok(hosts.length >= 2);
});

test('remote capability covers every site host', () => {
  const urls = JSON.parse(read('src-tauri/capabilities/default.json')).remote.urls;
  assert.deepEqual(urls, hosts.map(host => `https://${host}/*`));
});

for (const script of ['site-lifecycle', 'common-styles', 'segment-monitor', 'player-fullscreen', 'inject-common', 'restore-paused']) {
  test(`${script}.js origin guard lists every site host`, () => {
    const source = read(`src-tauri/scripts/${script}.js`);
    const list = source.match(/!\[([^\]]*)\]\.includes\(location\.origin\)/);
    assert.ok(list, 'origin guard not found');
    assert.deepEqual([...list[1].matchAll(/'([^']+)'/g)].map(match => match[1]), hosts.map(host => `https://${host}`));
  });
}
