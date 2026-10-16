const { invoke } = window.__TAURI__.core;
const buttons = [...document.querySelectorAll('[data-action]')];
const status = document.querySelector('#status');
let pending = false;

async function action(name) {
  if (pending) return;
  pending = true;
  buttons.forEach(button => { button.disabled = true; });
  try {
    await invoke('shikimori_action', { action: name });
    status.textContent = '';
  } catch (err) {
    status.textContent = String(err);
  } finally {
    pending = false;
    buttons.forEach(button => { button.disabled = false; });
  }
}

buttons.forEach(button => button.addEventListener('click', () => action(button.dataset.action)));
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') {
    event.preventDefault();
    return action('close');
  }
});
document.addEventListener('click', event => {
  if (event.target === document.body) action('close');
});
