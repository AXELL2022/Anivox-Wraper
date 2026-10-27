const { invoke } = window.__TAURI__.core;
const startup = document.querySelector('#startup-page');
const saveSession = document.querySelector('#save-session');
const sessionStatus = document.querySelector('#session-status');
let sessionBusy = false;
let sessionLoaded = false;
async function sessionAction(action) {
  if (sessionBusy) return;
  sessionBusy = true;
  startup.disabled = saveSession.disabled = true;
  try { await action(); }
  catch (err) { sessionStatus.textContent = `Не удалось сохранить/загрузить запуск: ${err}`; }
  finally {
    sessionBusy = false;
    startup.disabled = !sessionLoaded;
    saveSession.disabled = false;
  }
}
async function loadSession() {
  const info = await invoke('get_session_settings');
  if (!info || !['home', 'lastPage'].includes(info.startupPage)) throw new Error('Некорректные настройки запуска');
  startup.value = info.startupPage;
  sessionLoaded = true;
}
document.querySelector('#session-form').addEventListener('submit', event => {
  event.preventDefault();
  return sessionAction(async () => {
    if (!sessionLoaded) { await loadSession(); sessionStatus.textContent = 'Настройки загружены. Выберите вариант запуска.'; return; }
    const info = await invoke('save_session_settings', { startupPage: startup.value });
    startup.value = info.startupPage;
    sessionStatus.textContent = 'Вариант запуска сохранён.';
  });
});
sessionAction(loadSession);
const input = document.querySelector('#mpv-path');
const browse = document.querySelector('#browse');
const save = document.querySelector('#save');
const reset = document.querySelector('#reset');
const status = document.querySelector('#status');
const mirror = document.querySelector('#mirror-embedded');
const clearHistory = document.querySelector('#clear-history');
let busy = false;
function render(info) {
  input.value = info.mpvPath || '';
  mirror.checked = !!info.mirrorEmbedded;
  document.querySelector('#logs').textContent = info.logDirectory;
}
async function perform(action) {
  if (busy) return;
  busy = true;
  input.disabled = browse.disabled = save.disabled = reset.disabled = mirror.disabled = clearHistory.disabled = true;
  try { await action(); }
  catch (err) { status.textContent = String(err); }
  finally { busy = false; input.disabled = browse.disabled = save.disabled = reset.disabled = mirror.disabled = clearHistory.disabled = false; }
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
    const info = await invoke('save_player_settings_command', { mpvPath: input.value.trim() || null, mirrorEmbedded: mirror.checked });
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
// Панель не пересоздаётся: при повторном показе тихо подтягиваем значения из кеша приложения.
window.addEventListener('anivox-overlay-show', async () => {
  if (busy || sessionBusy) return;
  try {
    render(await invoke('get_player_settings'));
    if (sessionLoaded) startup.value = (await invoke('get_session_settings')).startupPage;
    status.textContent = '';
    sessionStatus.textContent = '';
  } catch (_) {}
});

clearHistory.addEventListener('click', () => perform(async () => {
  await invoke('clear_playback_history');
  status.textContent = 'Сохранённые позиции удалены.';
}));
