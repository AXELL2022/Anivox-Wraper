import { test } from 'node:test';
import assert from 'node:assert/strict';
import { localWindow } from './local_window_harness.mjs';

test('Settings loads path, preserves it on picker cancellation and saves explicit changes', async () => {
  const saved = [];
  const app = await localWindow('../ui/settings.js', async (command, args) => {
    if (command === 'get_player_settings') return { mpvPath: 'C:\\Плеер с пробелом\\mpv.exe', logDirectory: 'C:\\Anivox\\logs' };
    if (command === 'pick_mpv_executable') return null;
    if (command === 'save_player_settings_command') { saved.push(args.mpvPath); return { mpvPath: args.mpvPath, logDirectory: 'C:\\Anivox\\logs' }; }
  });
  assert.equal(app.element('#mpv-path').value, 'C:\\Плеер с пробелом\\mpv.exe');
  assert.equal(app.element('#logs').textContent, 'C:\\Anivox\\logs');
  await app.click('#browse');
  assert.equal(app.element('#mpv-path').value, 'C:\\Плеер с пробелом\\mpv.exe');
  assert.equal(saved.length, 0);
  await app.click('#reset');
  assert.equal(app.element('#mpv-path').value, '');
  assert.equal(saved.length, 0);
  await app.submit();
  assert.deepEqual(saved, [null]);
});

test('Settings errors preserve input and allow retry; Escape only closes the local window', async () => {
  let attempts = 0;
  let closed = 0;
  const app = await localWindow('../ui/settings.js', async command => {
    if (command === 'get_player_settings') throw new Error('bad settings.json');
    if (command === 'save_player_settings_command' && ++attempts === 1) throw new Error('access denied');
    if (command === 'close_local_window') closed++;
    return { mpvPath: 'C:\\mpv.exe', logDirectory: 'logs' };
  });
  assert.match(app.element('#status').textContent, /settings.json/);
  app.element('#mpv-path').value = 'C:\\mpv.exe';
  await app.submit();
  assert.equal(app.element('#mpv-path').value, 'C:\\mpv.exe');
  assert.equal(app.element('#save').disabled, false);
  assert.match(app.element('#status').textContent, /access denied/);
  await app.submit();
  assert.equal(attempts, 2);
  await app.keys.get('keydown')({ key: 'Escape' });
  assert.equal(closed, 1);
});

test('Session startup setting loads and saves independently of player settings', async () => {
  const saved = [];
  const app = await localWindow('../ui/settings.js', async (command, args) => {
    if (command === 'get_session_settings') return {startupPage:'home'};
    if (command === 'save_session_settings') { saved.push(args.startupPage); return args; }
    return {mpvPath:null,logDirectory:'logs'};
  });
  assert.equal(app.element('#startup-page').value,'home');
  app.element('#startup-page').value='lastPage';
  await app.element('#session-form').handlers.submit({preventDefault(){}});
  assert.deepEqual(saved,['lastPage']);
  assert.equal(app.element('#save-session').disabled,false);
});

test('Session failed load must be retried before it can overwrite the preference', async () => {
  let loads = 0, saves = 0;
  const app = await localWindow('../ui/settings.js', async command => {
    if (command === 'get_session_settings') { if (++loads === 1) throw new Error('disk failure'); return {startupPage:'home'}; }
    if (command === 'save_session_settings') { saves++; throw new Error('write denied'); }
    return {logDirectory:'logs'};
  });
  assert.equal(app.element('#startup-page').disabled,true);
  assert.match(app.element('#session-status').textContent,/disk failure/);
  await app.element('#session-form').handlers.submit({preventDefault(){}});
  assert.equal(saves,0);
  assert.equal(app.element('#startup-page').value,'home');
  await app.element('#session-form').handlers.submit({preventDefault(){}});
  assert.equal(saves,1);
  assert.match(app.element('#session-status').textContent,/write denied/);
  assert.equal(app.element('#save-session').disabled,false);
});

test('Settings exposes mirror preference and clears local playback history', async () => {
  const calls = [];
  const app = await localWindow('../ui/settings.js', async (command, args) => {
    calls.push({command,args});
    if (command === 'get_player_settings') return {mpvPath:null,mirrorEmbedded:true,logDirectory:'logs'};
    if (command === 'save_player_settings_command') return {...args,logDirectory:'logs'};
  });
  assert.equal(app.element('#mirror-embedded').checked,true);
  app.element('#mirror-embedded').checked=false;
  await app.submit();
  assert.equal(calls.find(c=>c.command==='save_player_settings_command').args.mirrorEmbedded,false);
  await app.click('#clear-history');
  assert.equal(calls.filter(c=>c.command==='clear_playback_history').length,1);
});
