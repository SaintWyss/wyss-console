// LAB driver: clicks the UI through the DOM and reports through lab_trace. Never sends OS input.
(() => {
  if (window.__labDriver) return;
  window.__labDriver = true;
  const inv = (c, a) => window.__TAURI_INTERNALS__.invoke(c, a);
  const log = (event, detail = '') =>
    inv('lab_trace', { event, detail: String(detail) }).catch(() => {});
  // Every IPC call the page makes (Tauri sends them with fetch): when it left, when the response
  // headers and the body arrived. The page's own promise settles after the body.
  const rawFetch = window.fetch.bind(window);
  let seq = 0;
  window.fetch = (input, init) => {
    const url = typeof input === 'string' ? input : (input && input.url) || '';
    if (!/ipc/.test(url) || /lab_trace/.test(url)) return rawFetch(input, init);
    const cmd = url.split('?')[0].split('/').pop();
    const id = ++seq, t = performance.now();
    log('ipc.begin', `#${id} ${cmd}`);
    return rawFetch(input, init).then(
      (r) => {
        log('ipc.headers', `#${id} ${cmd} ms=${Math.round(performance.now() - t)} status=${r.status}`);
        r.clone().arrayBuffer().then(() => log('ipc.body', `#${id} ${cmd} ms=${Math.round(performance.now() - t)}`), () => log('ipc.body', `#${id} ${cmd} unreadable`));
        return r;
      },
      (e) => { log('ipc.fail', `#${id} ${cmd} ms=${Math.round(performance.now() - t)} ${e && e.message}`); throw e; },
    );
  };
  const txt = (el) => (el ? el.textContent : '').replace(/\s+/g, ' ').trim();
  const all = (sel) => [...document.querySelectorAll(sel)];
  const btn = (needle, { exact = false, scope = '.runtimes-feature' } = {}) =>
    [...(document.querySelector(scope) || document).querySelectorAll('button, [role=tab], a')].find((e) => {
      const t = txt(e);
      return !e.disabled && (exact ? t === needle : t.includes(needle));
    });
  window.__labTtl = 60;
  const state = { step: 0, since: Date.now(), last: '' };
  const goto = (n, why) => {
    state.step = n;
    state.since = Date.now();
    log('step', `${n} ${why}`);
  };
  const banner = () => txt(document.querySelector('.runtimes-feature [role=status], .runtimes-feature .notice, .runtimes-feature .operation')) ;
  const dump = () => {
    const main = document.querySelector('.runtimes-feature');
    const t = main ? txt(main).slice(0, 700) : txt(document.body).slice(0, 300);
    if (t !== state.last) {
      state.last = t;
      log('dom', t);
    }
  };
  const tick = () => {
    try {
      dump();
      const waited = Date.now() - state.since;
      switch (state.step) {
        case 0: {
          const nav = btn('Runtimes', { scope: 'body' });
          if (nav) { nav.click(); goto(1, 'nav-runtimes'); }
          else if (waited > 15000) { log('buttons', all('button').map(txt).join(' | ')); goto(99, 'no-nav'); }
          break;
        }
        case 1: {
          // wait for the link to be connected
          if (/protocol 2/.test(txt(document.querySelector('.runtimes-feature')))) goto(2, 'connected');
          else if (waited > 15000) goto(99, 'not-connected');
          break;
        }
        case 2: {
          const card = all('.runtimes-feature button').find((b) => /Session [0-9a-f]{8}/.test(txt(b)));
          if (card) { card.click(); goto(4, 'selected-existing'); }
          else {
            const n = btn('New session');
            if (n) { n.click(); goto(3, 'new-session'); }
          }
          break;
        }
        case 3: {
          const card = all('.runtimes-feature button').find((b) => /Session [0-9a-f]{8}/.test(txt(b)));
          if (card) { card.click(); goto(4, 'selected-new'); }
          else if (waited > 10000) goto(99, 'no-card');
          break;
        }
        case 4: {
          const detail = () => /Start the runtime|Runtime /.test(txt(document.querySelector('.runtimes-feature')));
          if (!detail()) {
            const card = all('.runtimes-feature button').find((b) => /Session [0-9a-f]{8}/.test(txt(b)));
            if (card && waited > 800) { card.click(); state.since = Date.now(); }
            break;
          }
          log('controls', all('.runtimes-feature button, .runtimes-feature select, .runtimes-feature input').map((b) => `${b.tagName}[${b.type||''}|${b.name||b.id||''}|${txt(b).slice(0,40)}|${b.value||''}]${b.disabled ? '(off)' : ''}`).join(' ; '));
          goto(5, 'detail-open');
          break;
        }
        case 5: {
          const r = all('.runtimes-feature input[name=rt-mode]').find((i) => i.value === 'local_tools');
          if (r && !r.checked) r.click();
          if (!state.chose) {
            const c = btn('Choose folder');
            if (c) { c.click(); state.chose = true; log('chose-folder'); }
            break;
          }
          const start = btn('Start runtime');
          if (start) { start.click(); goto(6, 'start-clicked'); }
          else if (waited > 12000) { log('controls', all('.runtimes-feature button').map((b) => `${txt(b)}${b.disabled ? '(off)' : ''}`).join(' | ')); goto(99, 'start-disabled'); }
          break;
        }
        case 6: {
          const t = txt(document.querySelector('.runtimes-feature'));
          if (/Ready/.test(t) && !/Starting the runtime/.test(t)) goto(7, 'ready');
          else if (waited > 40000) { log('start.stuck', t.slice(0, 400)); goto(99, 'start-stuck'); }
          break;
        }
        case 7: {
          const g = btn('Grant lease');
          if (g && waited > 1500) {
            const sel = document.querySelector('.runtimes-feature .rt-select select');
            if (sel && window.__labTtl) { sel.value = String(window.__labTtl); sel.dispatchEvent(new Event('change', { bubbles: true })); }
            state.t = Date.now();
            g.click();
            g.click();
            g.click();
            state.ping = setInterval(() => {
              const t = performance.now();
              inv('f8_state').then(() => log('ping', `ms=${Math.round(performance.now() - t)}`), () => log('ping', 'err'));
            }, 1000);
            log('grant.click', `acts=${all('.runtimes-feature input[type=checkbox]').filter((c) => c.checked).length}`);
            goto(8, 'grant-clicked');
          } else if (waited > 15000) goto(99, 'no-grant-button');
          break;
        }
        case 8: {
          const t = txt(document.querySelector('.runtimes-feature'));
          const b = txt(document.querySelector('.runtimes-feature .sessions-status'));
          if (b !== state.banner) { state.banner = b; log('banner', b.slice(0, 200)); }
          if (!/Granting the execution lease|Waiting for your answer/.test(t)) {
            clearInterval(state.ping);
            log('grant.cleared', `ms=${Date.now() - state.t} text=${t.slice(0, 500)}`);
            goto(9, 'cleared');
          } else if (Date.now() - state.t > 60000) {
            log('grant.stuck', t.slice(0, 500));
            goto(99, 'stuck');
          }
          break;
        }
        case 9: {
          if (waited > 1500) {
            log('post-grant', txt(document.querySelector('.runtimes-feature')).slice(0, 1200));
            log('controls', all('.runtimes-feature button').map((b) => `${txt(b)}${b.disabled ? '(off)' : ''}`).join(' | '));
            state.leaseSeen = Date.now();
            goto(10, 'post-grant-dumped');
          }
          break;
        }
        case 10: {
          // Revoke (in-page confirmation first)
          const r = btn('Revoke');
          if (r) { r.click(); log('revoke.click'); goto(11, 'revoke-clicked'); }
          else if (waited > 8000) goto(99, 'no-revoke-button');
          break;
        }
        case 11: {
          if (waited > 700) {
            log('controls', 'after-revoke-click: ' + all('.runtimes-feature button').map((b) => `${txt(b)}${b.disabled ? '(off)' : ''}`).join(' | '));
            const c = all('.runtimes-feature button').find((b) => /^(Revoke lease|Confirm|Yes, revoke|Revoke now)/.test(txt(b)) && !b.disabled);
            if (c) { state.t2 = Date.now(); c.click(); log('revoke.confirm.click', txt(c)); goto(12, 'revoke-confirmed'); }
            else if (waited > 8000) goto(99, 'no-revoke-confirm');
          }
          break;
        }
        case 12: {
          const t = txt(document.querySelector('.runtimes-feature'));
          if (!/Revoking the execution lease/.test(t) && btn('Grant lease')) {
            log('revoke.cleared', `ms=${Date.now() - state.t2}`);
            goto(13, 'revoked');
          } else if (Date.now() - state.t2 > 30000) { log('revoke.stuck', t.slice(0, 400)); goto(99, 'revoke-stuck'); }
          break;
        }
        case 13: {
          // second lease, 1 minute, to watch it expire
          const g = btn('Grant lease');
          if (g && waited > 1200) {
            state.t = Date.now();
            g.click();
            log('grant2.click');
            goto(14, 'grant2-clicked');
          }
          break;
        }
        case 14: {
          const t = txt(document.querySelector('.runtimes-feature'));
          if (!/Granting the execution lease|Waiting for your answer/.test(t)) {
            state.granted2 = Date.now();
            log('grant2.cleared', `ms=${Date.now() - state.t}`);
            goto(15, 'waiting-expiry');
          } else if (Date.now() - state.t > 60000) goto(99, 'grant2-stuck');
          break;
        }
        case 15: {
          // the lease lives 60 s: the window must notice by itself
          const t = txt(document.querySelector('.runtimes-feature'));
          const live = !btn('Grant lease');
          if (state.live === undefined) state.live = live;
          if (!live) {
            log('expired.noticed', `after_s=${Math.round((Date.now() - state.granted2) / 1000)}`);
            goto(16, 'expired');
          } else if (Date.now() - state.granted2 > 120000) { log('expired.never', t.slice(0, 500)); goto(99, 'no-expiry'); }
          break;
        }
        case 16: {
          if (waited > 2000) { log('done', txt(document.querySelector('.runtimes-feature')).slice(0, 600)); goto(99, 'finished'); }
          break;
        }
        default:
          break;
      }
    } catch (e) {
      log('driver-error', e && e.message);
    }
  };
  const boot = () => {
    log('driver.boot', location.href.slice(0, 60));
    setInterval(tick, 400);
  };
  if (document.readyState === 'complete') boot();
  else window.addEventListener('load', boot);
})();
