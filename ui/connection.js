const { invoke } = window.__TAURI__.core;
// Независимый быстрый CDN: измеряем реальную скорость скачивания пользователя.
const SPEED_URL = 'https://speed.cloudflare.com/__down?bytes=8000000';
const SPEED_LIMIT_MS = 6000;
const POLL_MS = 1000;
const STALE_MS = 4000;       // данные от сайта старше этого считаем отсутствующими
const GOOD_MBPS = 10;        // ниже этой скорости показатель красный
const STALL_MS = 5000;       // кусок видео без единого байта дольше этого — «завис»
const internetRow = document.querySelector('#check-internet');
const videoRow = document.querySelector('#check-video');
const videoDetail = document.querySelector('#video-detail');
const verdict = document.querySelector('#verdict');
const verdictTitle = document.querySelector('#verdict-title');
const verdictText = document.querySelector('#verdict-text');
const recheck = document.querySelector('#recheck');
const closer = document.querySelector('#close');
let net = null;        // { ok, mbps } — последнее измерение интернета
let measuring = false;
let seg = null;        // свежая статистика видео или null
let pollTimer = null;

const fmt = mbps => mbps >= 10 ? mbps.toFixed(0) : mbps.toFixed(1);

// Качаем тестовый файл потоком и считаем скорость.
async function measureInternet() {
  const controller = new AbortController();
  const started = performance.now();
  const timer = setTimeout(() => controller.abort(), SPEED_LIMIT_MS);
  let bytes = 0;
  try {
    const response = await fetch(`${SPEED_URL}&t=${Date.now()}`, { cache: 'no-store', signal: controller.signal });
    if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
    }
  } catch (_) {
    if (!bytes) return { ok: false, mbps: 0 };
  } finally {
    clearTimeout(timer);
  }
  return { ok: true, mbps: (bytes * 8) / ((performance.now() - started) / 1000) / 1e6 };
}

function paint(row, kind, text) {
  row.className = `check ${kind}`;
  row.querySelector('.check-state').textContent = text;
}

function setVerdict(kind, title, text) {
  verdict.className = `verdict ${kind}`;
  verdictTitle.textContent = title;
  verdictText.textContent = text;
}

// Состояние видео по свежим данным: none | stalled | failing | slow | ok | idle
function videoState() {
  if (!seg || !seg.active) return 'none';
  if (seg.inflight > 0 && seg.stalledMs >= STALL_MS) return 'stalled';
  if (seg.failed > 0 && seg.failed >= Math.max(1, seg.samples) / 2 && !seg.hasSpeed) return 'failing';
  if (!seg.hasSpeed) return 'idle';
  return seg.mbps < GOOD_MBPS ? 'slow' : 'ok';
}

function render() {
  const state = videoState();
  // Интернет не может быть медленнее, чем уже идущее видео.
  const netMbps = Math.max(net?.mbps ?? 0, state === 'ok' || state === 'slow' ? seg.mbps : 0);
  const netKnown = net !== null || state === 'ok';

  // --- интернет
  if (measuring && !netKnown) paint(internetRow, 'pending', 'Измеряем…');
  else if (net && !net.ok && netMbps === 0) paint(internetRow, 'bad', 'Нет связи');
  else if (netKnown) paint(internetRow, netMbps < GOOD_MBPS ? 'bad' : 'good', `${fmt(netMbps)} Мбит/с${measuring ? ' · измеряем…' : ''}`);

  // --- видео
  const host = seg?.host ? `Сервер: ${seg.host}` : '';
  switch (state) {
    case 'none':
      paint(videoRow, 'pending', 'Видео не идёт');
      videoDetail.hidden = true;
      break;
    case 'stalled':
      paint(videoRow, 'bad', `Завис ${Math.round(seg.stalledMs / 1000)} с`);
      break;
    case 'failing':
      paint(videoRow, 'bad', 'Ошибки загрузки');
      break;
    case 'idle':
      paint(videoRow, 'pending', 'Идёт буферизация…');
      break;
    case 'slow':
      paint(videoRow, 'bad', `${fmt(seg.mbps)} Мбит/с`);
      break;
    default:
      paint(videoRow, 'good', `${fmt(seg.mbps)} Мбит/с`);
  }
  if (state !== 'none') {
    const parts = [host, `в загрузке: ${seg.inflight}`];
    if (seg.slow) parts.push(`медленных (>8 с): ${seg.slow}`);
    if (seg.failed) parts.push(`ошибок: ${seg.failed}`);
    videoDetail.textContent = parts.filter(Boolean).join(' · ');
    videoDetail.hidden = false;
  }

  // --- вывод
  if (state === 'stalled' || state === 'failing') {
    const where = host ? ` (${seg.host})` : '';
    if (netKnown && netMbps >= GOOD_MBPS && !(net && !net.ok)) {
      setVerdict('warn', 'Тормозит видео-сервер, а не ваш интернет',
        `Интернет даёт ${fmt(netMbps)} Мбит/с, а кусок видео${where} ${state === 'stalled' ? `не приходит уже ${Math.round(seg.stalledMs / 1000)} с` : 'не загружается'}. Попробуйте позже, другую озвучку или VPN.`);
    } else if (measuring || !netKnown) {
      setVerdict('pending', 'Видео не загружается', 'Измеряем ваш интернет, чтобы понять, чья это проблема…');
    } else {
      setVerdict('bad', 'Проблема с вашим интернетом',
        net && !net.ok ? 'Не удалось скачать даже тестовый файл. Проверьте соединение, VPN и прокси.'
          : `Ваш канал даёт всего ${fmt(netMbps)} Мбит/с — видео не успевает загружаться.`);
    }
    return;
  }
  if (measuring && !netKnown) {
    setVerdict('pending', 'Проверяем…', 'Измеряем скорость интернета.');
    return;
  }
  if (net && !net.ok && netMbps === 0) {
    setVerdict('bad', 'Проблема с вашим интернетом',
      'Не удалось скачать тестовый файл с быстрого сервера. Проверьте соединение, VPN и прокси.');
    return;
  }
  if (state === 'none') {
    setVerdict(netMbps < GOOD_MBPS ? 'bad' : 'pending', netMbps < GOOD_MBPS ? 'Ваш интернет медленный' : 'Интернет в порядке',
      `Скорость ${fmt(netMbps)} Мбит/с. Включите серию — панель покажет состояние видео в реальном времени.`);
  } else if (state === 'idle') {
    setVerdict('pending', 'Видео загружается…', 'Данные ещё накапливаются, подождите пару секунд.');
  } else if (state === 'slow') {
    if (netMbps >= GOOD_MBPS && seg.mbps < netMbps * 0.4) {
      setVerdict('warn', 'Тормозит видео-сервер, а не ваш интернет',
        `Интернет даёт ${fmt(netMbps)} Мбит/с, а видео приходит лишь ${fmt(seg.mbps)} Мбит/с. Попробуйте позже, другую озвучку или VPN.`);
    } else {
      setVerdict('bad', 'Проблема с вашим интернетом',
        `Канал даёт ${fmt(netMbps)} Мбит/с, видео приходит ${fmt(seg.mbps)} Мбит/с — для плавного просмотра мало.`);
    }
  } else {
    setVerdict('good', 'Всё в порядке', `Видео приходит со скоростью ${fmt(seg.mbps)} Мбит/с — воспроизведение должно идти плавно.`);
  }
}

async function poll() {
  try {
    const stats = await invoke('get_segment_stats');
    seg = stats && Math.abs(Date.now() - stats.updatedAt) < STALE_MS ? stats : null;
  } catch (_) { seg = null; }
  render();
}

async function measure() {
  if (measuring) return;
  measuring = true;
  recheck.disabled = true;
  render();
  try { net = await measureInternet(); }
  finally { measuring = false; recheck.disabled = false; render(); }
}

function start() {
  clearInterval(pollTimer);
  pollTimer = setInterval(poll, POLL_MS);
  poll();
}

async function close() {
  try { await invoke('close_local_window'); }
  catch (err) { setVerdict('bad', 'Не удалось закрыть окно', String(err)); }
}

recheck.addEventListener('click', () => { net = null; measure(); });
closer.addEventListener('click', close);
document.addEventListener('keydown', event => { if (event.key === 'Escape') return close(); });
// Панель может быть скрыта и показана снова — перезапускаем и опрос, и измерение.
window.addEventListener('anivox-overlay-show', () => { net = null; start(); measure(); });
start();
measure();
