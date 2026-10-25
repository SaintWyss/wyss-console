(() => {
  if (window.__labDriver) return;
  window.__labDriver = true;
  const inv = (c, a) => window.__TAURI_INTERNALS__.invoke(c, a);
  const log = (event, detail = '') => inv('lab_trace', { event, detail: String(detail) }).catch(() => {});
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const txt = (el) => (el ? el.textContent : '').replace(/\s+/g, ' ').trim();
  const root = () => document.querySelector('.runtimes-feature');
  const btn = (n) => [...root().querySelectorAll('button')].find((b) => !b.disabled && txt(b).includes(n));
  const until = async (f, ms, label) => { const t = Date.now(); for (;;) { const v = f(); if (v) return v; if (Date.now() - t > ms) { log('timeout', label + ' :: ' + txt(root()).slice(0, 300)); throw new Error(label); } await sleep(150); } };
  const main = async () => {
    const nav = await until(() => [...document.querySelectorAll('button, a')].find((e) => txt(e) === 'Runtimes'), 20000, 'nav');
    nav.click();
    await until(() => /protocol 2/.test(txt(root())), 15000, 'connected');
    (await until(() => btn('New session'), 5000, 'new')).click();
    await sleep(900);
    if (!/Runtime /.test(txt(root()))) [...root().querySelectorAll('button')].find((b) => /Session [0-9a-f]{8}/.test(txt(b))).click();
    await until(() => /Runtime /.test(txt(root())), 8000, 'detail');
    [...root().querySelectorAll('input[name=rt-mode]')].find((i) => i.value === 'inference').click();
    await sleep(300);
    (await until(() => btn('Start runtime'), 5000, 'start')).click();
    await until(() => /try another mode|should not have|could not be started|exited/.test(txt(root().querySelector('.sessions-status'))), 15000, 'answer');
    log('inference.answer', txt(root().querySelector('.sessions-status')));
    log('driver.done');
  };
  const boot = () => main().catch((e) => log('driver-error', e && e.message));
  if (document.readyState === 'complete') boot(); else window.addEventListener('load', boot);
})();
