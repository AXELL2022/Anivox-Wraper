const { invoke } = window.__TAURI__.core;
const input = document.querySelector('#mpv-path');
const browse = document.querySelector('#browse');
const save = document.querySelector('#save');
const reset = document.querySelector('#reset');
const status = document.querySelector('#status');
let busy = false;
function render(info) {
  input.value = info.mpvPath || '';
  document.querySelector('#logs').textContent = info.logDirectory;
}
async function perform(action) {
  if (busy) return;
  busy = true;
  input.disabled = browse.disabled = save.disabled = reset.disabled = true;
  try { await action(); }
  catch (err) { status.textContent = String(err); }
  finally { busy = false; input.disabled = browse.disabled = save.disabled = reset.disabled = false; }
}
browse.addEventListener('click', () => perform(async () => {
  const path = await invoke('pick_mpv_executable');
  if (path) { input.value = path; status.textContent = 'Нажмите «Сохранить», чтобы применить путь.'; }
}));
reset.addEventListener('click', () => {
  if (busy) return;
  input.value = '';
  status.textContent = 'Нажмите «Сохранить», чтобы использовать MPV из PATH.';
});
document.querySelector('#settings-form').addEventListener('submit', event => {
  event.preventDefault();
  return perform(async () => {
    const info = await invoke('save_player_settings_command', { mpvPath: input.value.trim() || null });
    render(info);
    status.textContent = 'Настройки сохранены.';
  });
});
async function close() {
  try { await invoke('close_local_window'); }
  catch (err) { status.textContent = String(err); }
}
document.querySelector('#close').addEventListener('click', close);
document.addEventListener('keydown', event => { if (event.key === 'Escape') return close(); });
perform(async () => { render(await invoke('get_player_settings')); });
