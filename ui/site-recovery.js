(() => {
  'use strict';

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const title = document.querySelector('#title');
  const description = document.querySelector('#description');
  const detail = document.querySelector('#detail');
  const spinner = document.querySelector('#spinner');
  const actionError = document.querySelector('#action-error');
  const retry = document.querySelector('#retry');
  const home = document.querySelector('#home');
  const wait = document.querySelector('#wait');
  const buttons = [retry, home, wait];
  let pending = false;
  let eventSequence = 0;
  let generation = -1;

  function showError(prefix, error) {
    actionError.textContent = `${prefix}: ${String(error)}`;
    actionError.hidden = false;
  }

  function render(state) {
    if (!state || !['loading', 'ready', 'timeout', 'error'].includes(state.phase)
      || !Number.isSafeInteger(state.generation) || state.generation < generation) return;
    generation = state.generation;
    spinner.hidden = state.phase !== 'loading';
    const copy = {
      loading: ['Загрузка Anivox…', 'Подключаемся к сайту. Это может занять немного времени.'],
      ready: ['Anivox готов', 'Возвращаемся к просмотру…'],
      timeout: ['Сайт не ответил вовремя', 'Сайт или сеть могут работать медленно. Можно повторить загрузку, открыть главную или продолжить ожидание.'],
      error: ['Не удалось загрузить Anivox', 'При загрузке страницы произошла ошибка. Попробуйте ещё раз или перейдите на главную.'],
    };
    [title.textContent, description.textContent] = copy[state.phase];
    detail.textContent = typeof state.detail === 'string' ? state.detail : '';
    detail.hidden = !detail.textContent;
  }

  async function action(name) {
    if (pending) return;
    pending = true;
    buttons.forEach(button => { button.disabled = true; });
    actionError.textContent = '';
    actionError.hidden = true;
    try {
      // The local backend owns navigation and overlay visibility, including wait.
      await invoke('site_recovery_action', { action: name });
    } catch (error) {
      showError('Не удалось выполнить действие', error);
    } finally {
      pending = false;
      buttons.forEach(button => { button.disabled = false; });
    }
  }

  retry.addEventListener('click', () => action('retry'));
  home.addEventListener('click', () => action('home'));
  wait.addEventListener('click', () => action('wait'));

  async function initialize() {
    try {
      // Subscribe before querying so no state transition can fall into the gap.
      await listen('site-load-state', event => {
        eventSequence++;
        render(event.payload);
      });
    } catch (error) {
      showError('Не удалось подписаться на состояние загрузки', error);
    }
    const querySequence = eventSequence;
    try {
      const state = await invoke('get_site_load_state');
      // Even a same-generation event is newer than this in-flight snapshot.
      if (querySequence === eventSequence) render(state);
    } catch (error) {
      if (querySequence === eventSequence) showError('Не удалось получить состояние загрузки', error);
    }
  }

  void initialize();
})();
