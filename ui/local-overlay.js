(() => {
  const panel = document.querySelector('#local-panel');
  const closer = document.querySelector('#close') || document.querySelector('#later');
  let anchor = 0.98;
  function position() {
    const width = window.innerWidth;
    const cardWidth = Math.min(560, Math.max(0, width - 16));
    const left = Math.max(8, Math.min(width - cardWidth - 8, width * anchor - cardWidth + 26));
    panel.style.left = `${left}px`;
    panel.style.width = `${cardWidth}px`;
    panel.style.setProperty('--arrow-x', `${Math.max(14, Math.min(cardWidth - 14, width * anchor - left))}px`);
  }
  window.__anivoxOverlayAnchor = value => {
    if (Number.isFinite(value)) anchor = Math.max(0, Math.min(1, value));
    position();
  };
  window.addEventListener('resize', position);
  // Панель живёт в скрытом webview; при повторном показе только обновляем положение и фокус.
  window.__anivoxOverlayShow = () => {
    position();
    panel.classList.add('ready');
    panel.focus();
    window.dispatchEvent(new Event('anivox-overlay-show'));
  };
  document.addEventListener('click', event => {
    if (event.target === document.body || event.target === document.documentElement) closer?.click();
  });
  document.addEventListener('keydown', event => {
    if (event.key !== 'Tab') return;
    const elements = [...panel.querySelectorAll('button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]')].filter(el => !el.hidden && el.getClientRects().length);
    const first = elements[0], last = elements[elements.length - 1];
    if (!first) { event.preventDefault(); panel.focus(); return; }
    if (event.shiftKey && (document.activeElement === first || document.activeElement === panel)) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && (document.activeElement === last || document.activeElement === panel)) { event.preventDefault(); first.focus(); }
  });
  position();
  // Показываем панель только после того, как нативная часть задала размеры и якорь.
  const reveal = () => requestAnimationFrame(() => requestAnimationFrame(() => { position(); panel.classList.add('ready'); panel.focus(); }));
  window.__TAURI__.core.invoke('local_overlay_ready').then(reveal).catch(error => {
    panel.classList.add('ready');
    document.querySelector('#status').textContent = `Не удалось разместить панель: ${error}`;
  });
})();
