const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const toolbarRoot = document.querySelector('#anivox-toolbar-host')?.shadowRoot ?? document;
const vpn = toolbarRoot.querySelector('#vpn');
const vpnLabel = toolbarRoot.querySelector('#vpn-label');
const mpv = toolbarRoot.querySelector('#mpv');
const status = toolbarRoot.querySelector('#status');
let vpnActive = false;
let vpnPending = false;
let mpvTimer;

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

async function init() {
  await listen('player-status', ({ payload }) => {
    clearTimeout(mpvTimer);
    mpv.textContent = payload.text;
    mpv.style.color = payload.color;
    mpv.disabled = !payload.resetAfterMs;
    mpvTimer = setTimeout(resetMpv, payload.resetAfterMs || 10000);
  });
  await syncVpn();
  setInterval(syncVpn, 10000);
}

init().catch(err => message(`Ошибка панели: ${err}`, true));
