(() => {
  const inv = (c, a) => window.__TAURI_INTERNALS__.invoke(c, a);
  const log = (event, detail = '') => inv('lab_trace', { event, detail: String(detail) }).catch(() => {});
  const txt = (el) => (el ? el.textContent : '').replace(/\s+/g, ' ').trim();
  window.addEventListener('load', () => {
    setTimeout(() => {
      const nav = [...document.querySelectorAll('button, a')].find((e) => txt(e) === 'Runtimes');
      if (nav) nav.click();
    }, 400);
    setTimeout(() => log('readonly.dom', txt(document.querySelector('.runtimes-feature')).slice(0, 700)), 3000);
    setTimeout(() => log('readonly.done'), 3500);
  });
})();
