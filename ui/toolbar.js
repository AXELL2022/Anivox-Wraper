const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const toolbarRoot = document.querySelector('#anivox-toolbar-host')?.shadowRoot ?? document;
const vpn = toolbarRoot.querySelector('#vpn');
const vpnLabel = toolbarRoot.querySelector('#vpn-label');
const mpv = toolbarRoot.querySelector('#mpv');
const shikimori = toolbarRoot.querySelector('#shikimori');
const updater = toolbarRoot.querySelector('#updater');
const status = toolbarRoot.querySelector('#status');
const brand = toolbarRoot.querySelector('.brand');
let vpnActive = false;
let vpnPending = false;
let updatePending = false;
let availableUpdate = null;
let mpvTimer;
let shikimoriPending = false;

function renderShikimori(visible) {
  shikimori.setAttribute('aria-pressed', String(visible));
  shikimori.title = visible ? 'Закрыть Shikimori' : 'Открыть Shikimori поверх сайта';
}

shikimori.addEventListener('click', async () => {
  if (shikimoriPending) return;
  shikimoriPending = true;
  shikimori.disabled = true;
  try {
    renderShikimori(await invoke('shikimori_action', { action: 'toggle' }));
  } catch (err) {
    message(`Не удалось открыть Shikimori: ${err}`, true);
  } finally {
    shikimoriPending = false;
    shikimori.disabled = false;
  }
});

function message(text, error = false) {
  status.textContent = text;
  status.title = text;
  status.classList.toggle('error', error);
}

function renderVpn(active) {
  vpnActive = active;
  vpn.setAttribute('aria-pressed', String(active));
  vpnLabel.textContent = vpnPending ? 'VPN · …' : `VPN · ${active ? 'ВКЛ' : 'ВЫКЛ'}`;
  vpn.disabled = vpnPending;
  vpn.title = vpnPending ? 'Переключение WireGuard…' : `WireGuard: ${active ? 'отключить' : 'подключить'}`;
}

async function syncVpn() {
  if (vpnPending) return;
  try {
    const active = await invoke('get_vpn_status');
    if (!vpnPending) renderVpn(active);
  } catch (err) {
    message(`Не удалось проверить VPN: ${err}`, true);
    vpn.disabled = false;
  }
}

async function action(action) {
  try {
    await invoke('browser_action', { action });
  } catch (err) {
    message(String(err), true);
  }
}

toolbarRoot.querySelectorAll('[data-action]').forEach(button => {
  button.addEventListener('click', () => action(button.dataset.action));
});

vpn.addEventListener('click', async () => {
  if (vpnPending) return;
  const target = !vpnActive;
  vpnPending = true;
  renderVpn(vpnActive);
  message('Переключение WireGuard…');
  try {
    const active = await invoke('toggle_vpn', { enable: target });
    renderVpn(active);
    if (active !== target) throw new Error('WireGuard не подтвердил переключение');
    message(active ? 'VPN подключён. Перезагрузка сайта…' : 'VPN отключён. Перезагрузка сайта…');
    await action('reload');
  } catch (err) {
    message(`Ошибка WireGuard: ${err}`, true);
  } finally {
    vpnPending = false;
    renderVpn(vpnActive);
    await syncVpn();
  }
});

function resetMpv() {
  mpv.textContent = '▶ MPV';
  mpv.style.color = '';
  mpv.disabled = false;
}

mpv.addEventListener('click', async () => {
  mpv.disabled = true;
  mpv.textContent = 'MPV · …';
  clearTimeout(mpvTimer);
  mpvTimer = setTimeout(() => {
    resetMpv();
    message('MPV: сначала загрузите сайт и включите серию.', true);
  }, 10000);
  await action('mpv');
});

function showUpdateBadge(version, body) {
  if (!updater) return;
  availableUpdate = { version, body };
  if (updatePending) return;
  const cleanVersion = String(version || '').replace(/^v/, '');
  updater.style.display = 'inline-flex';
  updater.textContent = `✨ v${cleanVersion}`;
  updater.title = body ? `Доступно обновление v${cleanVersion}\n\n${body}` : `Доступно обновление v${cleanVersion}. Нажмите для установки.`;
}

if (updater) {
  updater.addEventListener('click', async () => {
    if (updatePending) return;
    updatePending = true;
    updater.disabled = true;
    updater.textContent = 'Загрузка… 0%';
    message('Загрузка обновления…');
    try {
      const result = await invoke('install_update');
      message(result === 'release_page'
        ? 'Открыта страница релиза. Установите обновление вручную.'
        : 'Обновление больше не доступно. Повторите проверку позже.');
    } catch (err) {
      message(`Ошибка обновления: ${err}`, true);
    } finally {
      updatePending = false;
      updater.disabled = false;
      if (availableUpdate) showUpdateBadge(availableUpdate.version, availableUpdate.body);
    }
  });
}

async function init() {
  await listen('shikimori-status', ({ payload }) => renderShikimori(payload.visible));
  invoke('shikimori_action', { action: 'status' }).then(renderShikimori).catch(() => {});

  await listen('player-status', ({ payload }) => {
    clearTimeout(mpvTimer);
    mpv.textContent = payload.text;
    mpv.style.color = payload.color;
    mpv.disabled = !payload.resetAfterMs;
    mpvTimer = setTimeout(resetMpv, payload.resetAfterMs || 10000);
  });

  await listen('update-available', ({ payload }) => {
    if (payload && payload.version) {
      showUpdateBadge(payload.version, payload.body);
    }
  });

  await listen('update-progress', ({ payload }) => {
    if (updater && updatePending) {
      const percent = payload.percent ?? 0;
      updater.textContent = `Загрузка… ${percent}%`;
      message(`Загрузка обновления: ${percent}%`);
    }
  });

  await listen('update-finished', () => {
    if (updater && updatePending) {
      updater.textContent = 'Перезапуск…';
      message('Обновление готово. Перезапуск…');
    }
  });

  async function syncUpdate() {
    try {
      const info = await invoke('check_update');
      if (info && info.available) {
        showUpdateBadge(info.version, info.body);
      }
    } catch (_) {}
  }
  syncUpdate();

  async function syncVersion() {
    try {
      const v = await invoke('get_app_version');
      if (v && brand) {
        brand.textContent = v;
      }
    } catch (_) {}
  }
  syncVersion();

  await syncVpn();
  setInterval(syncVpn, 10000);
}

init().catch(err => message(`Ошибка панели: ${err}`, true));

