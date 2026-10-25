// LAB driver (fixture runtime, in-process authority): bind, synthetic turns, approvals, stop/restart.
(() => {
  if (window.__labDriver) return;
  window.__labDriver = true;
  const inv = (c, a) => window.__TAURI_INTERNALS__.invoke(c, a);
  const log = (event, detail = '') => inv('lab_trace', { event, detail: String(detail) }).catch(() => {});
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const txt = (el) => (el ? el.textContent : '').replace(/\s+/g, ' ').trim();
  const root = () => document.querySelector('.runtimes-feature');
  const all = (sel) => [...(root() || document).querySelectorAll(sel)];
  const btn = (needle, scope) =>
    [...((scope && document.querySelector(scope)) || root() || document).querySelectorAll('button, [role=tab]')].find(
      (e) => !e.disabled && txt(e).includes(needle),
    );
  const until = async (f, ms, label) => {
    const t = Date.now();
    for (;;) {
      const v = f();
      if (v) return v;
      if (Date.now() - t > ms) { log('timeout', `${label} :: ${txt(root()).slice(0, 500)}`); throw new Error('timeout ' + label); }
      await sleep(150);
    }
  };
  const controls = (tag) => log('controls', `${tag}: ` + all('button, textarea, input').map((b) => `${b.tagName[0]}[${txt(b).slice(0, 30)}|${b.name || b.id || ''}]${b.disabled ? '(off)' : ''}`).join(' ; '));
  const dom = (tag, n = 900) => log('dom', `${tag}: ${txt(root()).slice(0, n)}`);
  const pendingText = () => /Granting|Waiting for your answer|Starting the runtime|Stopping|Binding|Asking the authority|Waiting for the runtime/.test(txt(root().querySelector('.sessions-status')));
  const idle = () => until(() => !pendingText(), 60000, 'idle');
  const setText = (el, value) => {
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const rawFetch = window.fetch.bind(window);
  let seq = 0;
  window.fetch = (input, init) => {
    const url = typeof input === 'string' ? input : (input && input.url) || '';
    if (!/ipc/.test(url) || /lab_trace/.test(url)) return rawFetch(input, init);
    const cmd = url.split('?')[0].split('/').pop();
    if (!/f8_(lease|runtime|binding|turn_start|turn_cancel|approval_(accept|decline|cancel)|session_(prepare|close))/.test(cmd)) return rawFetch(input, init);
    const id = ++seq, t = performance.now();
    log('ipc.begin', `#${id} ${cmd}`);
    return rawFetch(input, init).then((r) => { log('ipc.headers', `#${id} ${cmd} ms=${Math.round(performance.now() - t)} status=${r.status}`); return r; });
  };

  const main = async () => {
    await until(() => [...document.querySelectorAll('button, a')].find((e) => txt(e) === 'Runtimes'), 20000, 'nav');
    [...document.querySelectorAll('button, a')].find((e) => txt(e) === 'Runtimes').click();
    await until(() => /protocol 2/.test(txt(root())), 15000, 'connected');
    (await until(() => btn('New session'), 5000, 'new')).click();
    const card = await until(() => all('button').find((b) => /Session [0-9a-f]{8}/.test(txt(b))), 10000, 'card');
    await sleep(500);
    if (!/Runtime /.test(txt(root()))) card.click();
    await until(() => /Runtime /.test(txt(root())), 8000, 'detail');
    log('stage', 'session-open');
    // runtime
    all('input[name=rt-mode]').find((i) => i.value === 'local_tools').click();
    (await until(() => btn('Choose folder'), 5000, 'choose')).click();
    const start = await until(() => btn('Start runtime'), 8000, 'start-enabled');
    start.click();
    await until(() => /Ready · generation/.test(txt(root())) && !pendingText(), 40000, 'ready');
    log('stage', 'runtime-ready');
    // lease (all scopes, 15 min), allowed from inside the dialog
    const sel = root().querySelector('.rt-select select');
    if (sel) { sel.value = '900'; sel.dispatchEvent(new Event('change', { bubbles: true })); }
    (await until(() => btn('Grant lease'), 8000, 'grant')).click();
    await sleep(400); await idle();
    log('stage', 'lease-granted');
    controls('after-lease'); dom('after-lease', 1400);
    // bind workspace
    const choose = await until(() => all('button').filter((b) => txt(b).includes('Choose folder') || txt(b).includes('Change folder')).find((b) => !b.disabled), 8000, 'bind-choose');
    choose.click(); await sleep(500);
    (await until(() => btn('Bind workspace'), 8000, 'bind-enabled')).click();
    await sleep(400); await idle();
    log('stage', 'bound'); controls('after-bind'); dom('after-bind', 1400);
    // conversation tab
    (await until(() => btn('Conversation'), 5000, 'conv-tab')).click();
    await sleep(600); controls('conversation'); dom('conversation', 1400);
    const area = await until(() => all('textarea')[0], 8000, 'composer');
    setText(area, 'hello fixture');
    await sleep(300);
    const send = await until(() => all('button').find((b) => /^Run/.test(txt(b)) && !b.disabled), 6000, 'send');
    log('send.click', txt(send)); send.click();
    await until(() => /fixture runtime, not Codex/.test(txt(root())), 30000, 'turn-answer');
    log('stage', 'turn-answered'); dom('turn-answer', 1400);
    await sleep(1500);
    // a turn that asks to run a command: refuse it
    setText(all('textarea')[0], 'ask to run one command');
    await sleep(300);
    (await until(() => all('button').find((b) => /^Run/.test(txt(b)) && !b.disabled), 15000, 'send2')).click();
    await until(() => /wants to run|Waiting for approval|Decline/.test(txt(root())), 30000, 'approval');
    log('stage', 'approval-shown'); controls('approval'); dom('approval', 1600);
    const decline = await until(() => btn('Decline'), 8000, 'decline-btn');
    decline.click();
    await sleep(2500); log('stage', 'declined'); dom('after-decline', 1400); controls('after-decline');
    // cancel the running turn
    (await until(() => btn('Cancel turn'), 8000, 'cancel-turn')).click();
    await sleep(2500); log('stage', 'turn-cancelled'); dom('after-cancel-turn', 1200); controls('after-cancel-turn');
    // another turn that asks: this time the user allows it (native window, answered from inside)
    setText(all('textarea')[0], 'ask to run one command again');
    await sleep(300);
    (await until(() => btn('Run'), 15000, 'send3')).click();
    await until(() => btn('Accept'), 30000, 'approval2');
    log('stage', 'approval2-shown');
    btn('Accept').click();
    await sleep(600); log('banner-accept', txt(document.querySelector('.approval-panel, .approval, [class*=approval]') || root()).slice(0, 400));
    await until(() => !/Waiting for your Allow/.test(txt(root())), 30000, 'accept-settled');
    await sleep(1500); log('stage', 'accepted'); dom('after-accept', 1500); controls('after-accept');
    // stopping under a running turn is refused; cancel the turn, then stop
    (await until(() => btn('Overview'), 5000, 'ov0')).click(); await sleep(500);
    (await until(() => btn('Stop runtime'), 8000, 'stop-a')).click(); await sleep(500);
    (await until(() => btn('Stop runtime'), 6000, 'stop-b')).click(); await sleep(1500);
    log('stage', 'stop-under-turn-answer'); dom('stop-under-turn', 700);
    const dismiss = btn('Dismiss'); if (dismiss) dismiss.click();
    (await until(() => btn('Conversation'), 5000, 'conv2')).click(); await sleep(600);
    (await until(() => btn('Cancel turn'), 8000, 'cancel-turn2')).click(); await sleep(2500);
    // stop the runtime (two steps in the page), then start it again
    (await until(() => btn('Overview'), 5000, 'ov')).click(); await sleep(600);
    controls('overview-before-stop');
    (await until(() => btn('Stop runtime'), 8000, 'stop1')).click();
    await sleep(700); controls('stop-confirm');
    const stop2 = await until(() => btn('Stop runtime'), 6000, 'stop2');
    stop2.click(); await sleep(400); await until(() => /Stopped/.test(txt(root())) && !pendingText(), 30000, 'stopped');
    log('stage', 'stopped'); dom('after-stop', 1000);
    all('input[name=rt-mode]').find((i) => i.value === 'local_tools').click();
    (await until(() => btn('Choose folder') || btn('Change folder'), 5000, 'choose2')).click();
    (await until(() => btn('Start runtime'), 8000, 'start2')).click();
    await until(() => /Ready · generation 2/.test(txt(root())) && !pendingText(), 40000, 'ready2');
    log('stage', 'restarted-gen2'); dom('after-restart', 900);
    // capacity one: a second session may not start a runtime while the first is ready
    (await until(() => btn('New session'), 5000, 'new2')).click(); await sleep(1500);
    const cards = all('button').filter((b) => /Session [0-9a-f]{8}/.test(txt(b)) && !/×/.test(txt(b)));
    log('cards', cards.map(txt).join(' | '));
    const second = cards.find((b) => /Stopped/.test(txt(b)));
    if (second) { await sleep(500); if (!/Runtime (Stopped|Ready)/.test(txt(root()))) { second.click(); await sleep(800); } } else log('no-second-card');
    log('second-detail', txt(root()).slice(0, 300));
    all('input[name=rt-mode]').find((i) => i.value === 'local_tools')?.click();
    (await until(() => btn('Choose folder') || btn('Change folder'), 5000, 'choose3')).click();
    (await until(() => btn('Start runtime'), 8000, 'start3')).click();
    await sleep(3500); log('stage', 'second-start-answer'); dom('second-start', 1000);
    log('driver.done');
  };
  const boot = () => main().catch((e) => log('driver-error', e && e.message));
  if (document.readyState === 'complete') boot(); else window.addEventListener('load', boot);
})();
