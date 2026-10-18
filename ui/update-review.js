const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const install = document.querySelector('#install');
const later = document.querySelector('#later');
const retry = document.querySelector('#retry');
const status = document.querySelector('#status');
let version = null;
let pending = false;
install.disabled = true;
function render(info) {
  version = info.available ? info.version : null;
  document.querySelector('#version').textContent = version ? `Anivox v${version}` : 'Обновление Anivox';
  document.querySelector('#notes').textContent = info.body || 'Описание изменений не указано.';
  install.disabled = !version;
}
async function check() {
  if (pending) return;
  pending = true;
  install.disabled = retry.disabled = true;
  status.textContent = 'Проверка обновлений…';
  try {
    const info = await invoke('check_update');
    render(info);
    status.textContent = version ? 'Перед установкой ознакомьтесь с изменениями.' : 'Обновление больше не доступно.';
  } catch (err) { status.textContent = String(err); version = null; }
  finally { pending = false; retry.disabled = false; install.disabled = !version; }
}
install.addEventListener('click', async () => {
  if (pending || !version) return;
  pending = true;
  install.disabled = retry.disabled = later.disabled = true;
  status.textContent = 'Проверка выбранной версии…';
  try {
    const result = await invoke('install_update', { expectedVersion: version });
    if (result?.update_changed) {
      render({ available: true, ...result.update_changed });
      status.textContent = 'Доступна новая версия. Ознакомьтесь с изменениями и подтвердите установку.';
    } else {
      version = null;
      status.textContent = result === 'release_page'
        ? 'Открыта страница релиза. Установите обновление вручную.'
        : 'Обновление больше не доступно.';
    }
  } catch (err) { status.textContent = `Ошибка обновления: ${err}`; }
  finally { pending = false; install.disabled = !version; retry.disabled = later.disabled = false; }
});
async function close() {
  if (pending) return;
  try { await invoke('close_local_window'); } catch (err) { status.textContent = String(err); }
}
later.addEventListener('click', close);
retry.addEventListener('click', check);
document.addEventListener('keydown', event => { if (event.key === 'Escape') return close(); });
async function init() {
  await listen('update-progress', ({ payload }) => {
    if (pending) status.textContent = `Загрузка обновления: ${payload.percent ?? 0}%`;
  });
  await listen('update-finished', () => { if (pending) status.textContent = 'Обновление готово. Перезапуск…'; });
  await check();
}
init().catch(err => { status.textContent = String(err); });
