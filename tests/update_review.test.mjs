import { test } from 'node:test';
import assert from 'node:assert/strict';
import { localWindow } from './local_window_harness.mjs';

test('Update review displays notes as text; Later and Escape never install', async () => {
  let installs = 0;
  let closed = 0;
  const app = await localWindow('../ui/update-review.js', async command => {
    if (command === 'check_update') return { available: true, version: '0.2.5', body: '<script>test</script>\n* Changes' };
    if (command === 'install_update') installs++;
    if (command === 'close_local_window') closed++;
  });
  assert.equal(app.element('#notes').textContent, '<script>test</script>\n* Changes');
  assert.equal(installs, 0);
  await app.click('#later');
  await app.keys.get('keydown')({ key: 'Escape' });
  assert.equal(closed, 2);
  assert.equal(installs, 0);
});

test('New release between review and install requires another confirmation', async () => {
  const versions = [];
  const app = await localWindow('../ui/update-review.js', async (command, args) => {
    if (command === 'check_update') return { available: true, version: '0.2.5', body: 'Old notes' };
    if (command === 'install_update') {
      versions.push(args.expectedVersion);
      return versions.length === 1 ? { update_changed: { version: '0.2.6', body: 'New notes' } } : 'no_update';
    }
  });
  await app.click('#install');
  assert.deepEqual(versions, ['0.2.5']);
  assert.equal(app.element('#notes').textContent, 'New notes');
  assert.equal(app.element('#install').disabled, false);
  await app.click('#install');
  assert.deepEqual(versions, ['0.2.5', '0.2.6']);
  assert.equal(app.element('#install').disabled, true);
});

test('Review prevents overlapping installs, shows progress and recovers after failure', async () => {
  let reject;
  let calls = 0;
  const app = await localWindow('../ui/update-review.js', async command => {
    if (command === 'check_update') return { available: true, version: '0.2.5', body: null };
    if (command === 'install_update') { calls++; return await new Promise((_, fail) => { reject = fail; }); }
  });
  assert.match(app.element('#notes').textContent, /не указано/);
  const pending = app.click('#install');
  await app.click('#install');
  assert.equal(calls, 1);
  app.listeners.get('update-progress')({ payload: { percent: 42 } });
  assert.match(app.element('#status').textContent, /42%/);
  reject(new Error('Download failed'));
  await pending;
  assert.equal(app.element('#install').disabled, false);
  assert.match(app.element('#status').textContent, /Download failed/);
});

for (const outcome of ['release_page', 'no_update']) {
  test(`Review handles ${outcome} and disables installation`, async () => {
    const app = await localWindow('../ui/update-review.js', async command => command === 'check_update'
      ? { available: true, version: '0.2.5' } : outcome);
    await app.click('#install');
    assert.equal(app.element('#install').disabled, true);
    assert.match(app.element('#status').textContent, outcome === 'release_page' ? /страница релиза/ : /не доступно/);
  });
}
