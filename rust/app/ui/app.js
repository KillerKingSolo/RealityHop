// The Reality Hop window (the port of realityhop.pyw's App). Rust keeps the state and runs the actions; this page
// draws a snapshot after each change. No library, no network: it calls only Reality Hop's own commands, and every
// text goes in as text, never as markup.
'use strict';
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = id => document.getElementById(id);
let snap = null, sel = null, logShown = true, built = '';

// Hover help of every button, by its English text (TIPS)
const TIPS = {
  'Show candidates': 'Opens the nodes from the public node list that could replace yours. Ping and test them, then put them in slots.',
  'Fix: test online nodes': 'Replaces the selected node with the best node from the online list that passes a test.',
  'Fix: nodes that worked before': 'Replaces the selected node with the best node from the vault of nodes that worked before.',
  'Fix: quick swap, untested': 'Replaces the selected node at once with the next fastest node, without a test.',
  'Replace all with best 6': 'Tests the fastest nodes from the online list and your current ones, then puts the best 6 in. About a minute.',
  'Rescue from verified vault': 'For when every node is dead. Tests the vault of nodes that worked before and puts the best 6 in.',
  'Restart PC tunnel': 'Restarts the PC tunnel on the nodes in your slots. About 2 seconds.',
  'Run setup again': 'Opens the setup wizard again, to add a router or redo the VPN step or the background start. Your nodes are kept.',
  'Test on router': "The router's tunnel tests every node. If another PC changed the router, its nodes are taken first.",
  'Devices': 'Choose which devices on the router use the tunnel.',
  'Push to router': 'Sends the router config and the device list to the router and restarts its tunnel (a few seconds).',
  'Report a problem': 'Makes a redacted report of the logs to attach to a GitHub issue. Nothing is sent.',
  'Open backend folder': 'Opens the install folder with the PC, Router and logs folders.',
  'Open logs folder': 'Opens the folder with window.log, watch.log and core.log.',
  'Settings': 'Check interval, thresholds, router features, updates, the sing-box channel, uninstall.',
  'Check now': 'Tests every node through the running PC tunnel. If another PC changed the router, its nodes are taken first.',
  'Restart tunnel': 'Restarts the PC tunnel on the nodes in your slots. About 2 seconds.',
  'Cancel': 'Stops the action that is running.',
  'Stop': "Turns everything off, including the start at boot, until you press Start. Turn the VPN app's proxy off while stopped.",
  'Start': 'Turns the PC tunnel and the background watcher on again, now and at every boot.',
  'Update': 'Installs the new Reality Hop release. The tunnel keeps running.',
  'Hide': 'Hides the activity log.',
  'Show': 'Shows the activity log.',
};
const EVENTS = { drop: ['#ff4d4f', 'PIA Dropped'], back: ['#5fd38d', 'PIA Back'], replace: ['#f5a524', 'Node Replaced'],
  rescue: ['#b57cff', 'Rescue'], restart: ['#5b9cff', 'Tunnel Restart'], heal: ['#f5a524', 'Router Repaired'], error: ['#ff4d4f', 'error'] };
const HEAD = [['slot', 'SLOT'], ['server', 'SERVER'], ['front', 'FRONT'], ['where', 'LOCATION'], ['tcp', '443 PING'],
  ['tunnel', 'TUNNEL'], ['verdict', 'STATUS']];
const tcache = new Map();

// A text in the person's language, {name} fields filled from kw
async function T(s, kw) {
  const key = kw ? s + '\u0000' + JSON.stringify(kw) : s;
  if (!tcache.has(key)) tcache.set(key, await invoke('text', { s, kw: kw || null }));
  return tcache.get(key);
}

// An element with a class and text
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

// ---- hover help, shown after half a second (Tip)
let tipJob = null;
function tipFor(e, english) {
  e.addEventListener('mouseenter', () => {
    clearTimeout(tipJob);
    if (!TIPS[english]) return;
    tipJob = setTimeout(async () => {
      const t = $('tip'), r = e.getBoundingClientRect();
      t.textContent = await T(TIPS[english]);
      t.hidden = false;
      const below = r.bottom + 4, h = t.offsetHeight;
      t.style.left = Math.min(r.left, innerWidth - t.offsetWidth - 8) + 'px';
      t.style.top = (below + h > innerHeight - 8 ? r.top - h - 4 : below) + 'px';
    }, 500);
  });
  for (const ev of ['mouseleave', 'mousedown']) e.addEventListener(ev, () => { clearTimeout(tipJob); $('tip').hidden = true; });
}

// ---- dialogs: a question with Yes / No, or a message with OK (messagebox)
function ask(title, text, buttons) {
  return new Promise(async resolve => {
    $('modalTitle').textContent = title;
    $('modalText').textContent = text;
    const b = $('modalButtons');
    b.replaceChildren();
    for (const [label, value, accent] of buttons) {
      const x = el('button', accent ? 'accent' : '', await T(label));
      x.onclick = () => { $('modal').hidden = true; resolve(value); };
      b.append(x);
    }
    $('modal').hidden = false;
  });
}
const yesNo = async (title, text) => ask(await T(title), text, [['No', false], ['Yes', true, true]]);
const info = async text => ask('Reality Hop', text, [['OK', true, true]]);

// Start an action, after asking when it needs it
function act(name, args) {
  return invoke('act', { name, args: args || null });
}

// ---- sidebar: every action, grouped
async function buildSide(m) {
  const key = JSON.stringify(m);
  if (built === key) return;
  built = key;
  const nav = $('groups');
  nav.replaceChildren();
  const group = async (title, glyph) => {
    const g = el('div', 'group');
    g.append(el('span', 'gi', glyph), el('span', 'gt', await T(title)));
    nav.append(g);
  };
  const item = async (english, fn) => {
    const b = el('div', 'nav');
    b.append(el('span', 'ni', '›'), el('span', 'nt', await T(english)));
    b.onclick = () => { if (!b.classList.contains('disabled')) fn(); };
    tipFor(b, english);
    nav.append(b);
    return b;
  };
  await group('Nodes', '');
  await item('Show candidates', notYet);
  await item('Fix: test online nodes', () => fixSelected('online'));
  await item('Fix: nodes that worked before', () => fixSelected('vault'));
  await item('Fix: quick swap, untested', () => fixSelected('quick'));
  await item('Replace all with best 6', replaceAll);
  await item('Rescue from verified vault', rescue);
  if (!m.ronly) {
    await group('PC tunnel', '');
    await item('Restart PC tunnel', () => act('restart_pc'));
    await item('Run setup again', notYet);
  }
  if (m.router) {
    await group('Router', '');
    await item('Test on router', () => act('test_router'));
    await item('Devices', notYet);
    await item('Push to router', pushRouter);
    if (m.ronly) await item('Run setup again', notYet);
  }
  await group(m.backend ? 'Files & help' : 'Help', '');
  await item('Report a problem', notYet);
  if (m.backend) {
    await item('Open backend folder', () => invoke('open_folder', { which: 'data' }));
    await item('Open logs folder', () => invoke('open_folder', { which: 'logs' }));
  }
  $('bootRow').hidden = m.ronly;
  $('bootText').textContent = await T(m.mac ? 'Start tunnel at login' : 'Start tunnel at boot');
}

// A part of the window that is not ported yet
function notYet() {
  info('This window is not in the Rust version yet.');
}

// ---- actions with their questions (replace_selected, replace_all, rescue, push_phone, power)
async function fixSelected(how) {
  const main = snap.lists.find(l => l.key === snap.main);
  if (!sel || !main || !main.rows.length) return info(await T('Select a node in one of the tables first.'));
  const i = main.rows.findIndex(r => r.tag === sel.tag);
  if (i < 0) return info(await T('Select a node in the node table first.'));
  const m = snap.mode, tag = sel.tag, cur = main.rows[i].server;
  const then = await T(m.ronly ? 'Then the router tunnel is updated.' : m.router ? 'Then the router and the PC tunnel are updated.' :
    'Then the PC tunnel is updated.');
  const top = await invoke('top');
  const text = {
    online: await T('Test the {top} fastest nodes from the online node list (under a minute) and swap the best one in for {tag} ({cur})?',
      { top, tag, cur }) + ' ' + then,
    vault: await T('Test the nodes that worked before (the vault) again and swap the best one in for {tag} ({cur})?', { tag, cur }) +
      ' ' + then + ' ' + await T('Useful when the online node list cannot be reached.'),
    quick: await T('Swap {tag} ({cur}) for the node that answers fastest on port 443 from this PC? Only the configs change: the node is not tested and nothing restarts until you say so.',
      { tag, cur }),
  }[how];
  if (await yesNo('Fix node', text)) act('fix', { slot: i, how });
}
async function replaceAll() {
  const top = await invoke('top');
  if (await yesNo('Replace all', await T('Scan the node list, test the {top} fastest candidates plus the current nodes on this PC, put the 6 best into the configs, and update the tunnel(s)?\n\nAbout a minute.', { top })))
    act('replace_all');
}
async function rescue() {
  if (await yesNo('Rescue', await T('For when every node is dead and nothing else works.\n\nThis PC tests the nodes in the vault (every node that ever passed a test), puts the best 6 into the configs, and updates the tunnel(s). If too few work, it scans the node list for more.')))
    act('rescue');
}
async function pushRouter() {
  if (await yesNo('Push to router', await T('Send router-config.json and the device list to the router over SSH and restart its sing-box? The router tunnel drops for a few seconds.')))
    act('push_router');
}
async function power() {
  const m = snap.mode, start = snap.stopped;
  const what = m.mac ? (start ? 'Start everything again on this Mac?\n\n- the PC tunnel (rh-core) starts now and at every login\n- the background watcher and menu bar icon start now and at every login\n\nAfterwards turn the VPN app\'s proxy back on (PIA: Settings → Multi-Hop, tick "Multi-hop and Obfuscation", SOCKS5 127.0.0.1:2080).'
    : 'Stop everything on this Mac, for when you are somewhere else?\n\n- the PC tunnel (rh-core) stops and no longer starts at login\n- the background watcher and menu bar icon stop and no longer start at login\n- the router and your configs are not touched\n\nAfterwards turn the VPN app\'s proxy off (PIA: Settings → Multi-Hop, untick "Multi-hop and Obfuscation"), or it cannot connect. Press Start to undo.')
    : (start ? 'Start everything again on this PC?\n\n- the PC tunnel (rh-core) starts now and at every boot (one UAC prompt)\n- the background watcher and tray icon start now and at every logon\n\nAfterwards turn the VPN app\'s proxy back on (PIA: Settings → Multi-Hop, tick "Multi-hop and Obfuscation", SOCKS5 127.0.0.1:2080).'
    : 'Stop everything on this PC, for when you are somewhere else?\n\n- the PC tunnel (rh-core) stops and no longer starts at boot (one UAC prompt)\n- the background watcher and tray icon stop and no longer start at logon\n- the router and your configs are not touched\n\nAfterwards turn the VPN app\'s proxy off (PIA: Settings → Multi-Hop, untick "Multi-hop and Obfuscation"), or it cannot connect. Press Start to undo.');
  if (await yesNo(start ? 'Start Reality Hop' : 'Stop Reality Hop', await T(what))) act('power', { stop: !start });
}

// ---- drawing
async function render(s) {
  snap = s;
  const m = s.mode;
  await buildSide(m);
  $('modeLine').textContent = (await T(m.ronly ? 'Router only' : m.router ? 'PC + router' : 'PC only')) + '  ·  ' +
    (s.core_mode || m.ronly ? s.version : await T('rh-core MISSING'));
  $('rcoreLine').textContent = [s.rcore, s.router_away ? await T('Router not connected') : ''].filter(Boolean).join('\n');
  $('rcoreLine').classList.toggle('away', !!s.router_away);
  $('autoOn').checked = s.auto_on;
  $('autoFix').checked = s.auto_fix;
  if (document.activeElement !== $('autoMin')) $('autoMin').value = s.check_min;
  $('bgOn').checked = s.startup_on && !s.stopped;
  $('bootOn').checked = s.boot_on;
  $('nextAuto').textContent = s.next_auto;
  // header
  const pill = $('pill');
  pill.className = 'pill ' + s.status.state;
  pill.textContent = s.status.pill;
  $('prog').hidden = !s.busy;
  $('cancelBtn').disabled = !s.busy;
  $('powerBtn').textContent = await T(s.stopped ? 'Start' : 'Stop');
  $('restartBtn').textContent = await T(m.ronly ? 'Test on router' : 'Restart tunnel');
  $('updateBtn').hidden = !s.update;
  if (s.update) $('updateBtn').textContent = await T('Update to {ver}', { ver: s.update });
  // tiles
  const tiles = $('tiles');
  tiles.style.gridTemplateColumns = `repeat(${s.tiles.length}, minmax(0, 1fr))`;
  tiles.replaceChildren(...s.tiles.map(t => {
    const d = el('div', 'tile');
    const sub = el('div', 'ts' + (t.act ? ' link' : ''), t.sub);
    if (t.act) sub.onclick = () => t.act === 'push_router' ? pushRouter() : act(t.act);
    d.append(el('div', 'tl', t.label), el('div', 'tv c-' + t.color, t.value), sub);
    return d;
  }));
  // status line
  $('status').textContent = s.status.text;
  $('status').className = s.status.state === 'ok' || s.status.state === 'unk' ? '' : 'c-' + s.status.state;
  $('statusBar').style.background = s.status.state === 'unk' ? 'var(--acc)' : `var(--${s.status.state})`;
  await drawHistory();
  await drawLists();
}

// The node tables (render)
async function drawLists() {
  const box = $('lists');
  const heads = await Promise.all(HEAD.map(([, h]) => T(h)));
  box.replaceChildren(...snap.lists.map(l => {
    const c = el('div', 'card list');
    const h = el('div', 'list-head');
    h.append(el('span', 'lt', l.label), el('span', 'lf', l.file), el('span', 'ln', l.note));
    const t = el('table');
    const cols = el('colgroup');
    for (const [k] of HEAD) cols.append(el('col', 'c-' + k));
    const thead = el('thead'), hr = el('tr');
    for (const x of heads) hr.append(el('th', '', x));
    thead.append(hr);
    const body = el('tbody');
    for (const r of l.rows) {
      const tr = el('tr', r.state + (sel && sel.key === l.key && sel.tag === r.tag ? ' sel' : ''));
      for (const v of [r.tag, r.server, r.front, r.where, r.tcp, r.tunnel, r.text]) tr.append(el('td', '', v));
      tr.onclick = e => { e.stopPropagation(); sel = { key: l.key, tag: r.tag }; drawLists(); };
      tr.oncontextmenu = e => { e.preventDefault(); e.stopPropagation(); sel = { key: l.key, tag: r.tag }; drawLists(); popup(e); };
      tr.ondblclick = () => details(l.key, r.tag);
      body.append(tr);
    }
    t.append(cols, thead, body);
    c.append(h, t);
    return c;
  }));
}

// The right-click menu of a node (popup)
async function popup(e) {
  const menu = $('menu');
  menu.replaceChildren();
  for (const [how, label] of [['online', 'Fix this node: test online nodes and swap in the best one'],
    ['vault', 'Fix this node: swap in a node that worked before (tested again)'],
    ['quick', 'Fix this node: quick swap to the fastest ping (configs only, not tested)']]) {
    const d = el('div', '', await T(label));
    d.onclick = () => { menu.hidden = true; fixSelected(how); };
    menu.append(d);
  }
  menu.hidden = false;
  menu.style.left = Math.min(e.clientX, innerWidth - menu.offsetWidth - 4) + 'px';
  menu.style.top = Math.min(e.clientY, innerHeight - menu.offsetHeight - 4) + 'px';
}

// The last 24 hours: PIA drops and returns, replacements, restarts, rescues (draw_history)
async function drawHistory() {
  const legend = $('legend');
  if (!legend.childElementCount) {
    for (const k of ['drop', 'replace', 'restart', 'rescue']) {
      const d = el('span', 'd', '●');
      d.style.color = EVENTS[k][0];
      legend.append(d, el('span', 'n', await T(EVENTS[k][1])));
    }
  }
  const c = $('hist'), w = c.clientWidth, h = 40, dpr = devicePixelRatio || 1;
  c.width = w * dpr; c.height = h * dpr;
  const g = c.getContext('2d');
  g.scale(dpr, dpr);
  g.strokeStyle = '#26262d';
  g.beginPath(); g.moveTo(0, h - 12.5); g.lineTo(w, h - 12.5); g.stroke();
  const L = 24, R = w - 24, span = 86400, now = snap.now;
  g.font = '7pt ' + getComputedStyle(document.body).fontFamily;
  g.fillStyle = '#5c5c66';
  g.textAlign = 'center';
  for (let hr = 0; hr <= 24; hr += 3) {
    const x = Math.round(R - hr / 24 * (R - L)) + .5;
    g.beginPath(); g.moveTo(x, h - 15); g.lineTo(x, h - 9); g.stroke();
    g.fillText(hr === 0 ? await T('now') : await T('-{n}h', { n: hr }), x, h - 1);
  }
  const counts = {};
  for (const [t, kind] of snap.events) {
    const col = (EVENTS[kind] || ['#8d8d97'])[0], x = R - (now - t) / span * (R - L), y = kind === 'drop' || kind === 'back' ? 14 : 6;
    g.strokeStyle = g.fillStyle = col;
    if (kind === 'back') { g.beginPath(); g.moveTo(x, y - 3); g.lineTo(x, y + 3); g.stroke(); }
    else { g.beginPath(); g.arc(x, y, 3, 0, 2 * Math.PI); g.fill(); }
    counts[kind] = (counts[kind] || 0) + 1;
  }
  const parts = [];
  for (const [k, e] of [['PIA Drops {n}', 'drop'], ['Replacements {n}', 'replace'], ['Restarts {n}', 'restart'], ['Rescues {n}', 'rescue']])
    parts.push(await T(k, { n: counts[e] || 0 }));
  $('histInfo').textContent = parts.join('   ·   ');
}

// One activity line, with its time (drain)
function logLine(at, line) {
  const log = $('log');
  const end = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
  log.append(el('span', 't', at + '  '), el('span', /^(error|FAIL)/.test(line) || line.includes('NOT ') ? 'err' : '', line + '\n'));
  while (log.childElementCount > 4000) log.firstChild.remove();   // a window left open for days: the last 2000 lines
  if (end) log.scrollTop = log.scrollHeight;
}

// Draw again from a fresh snapshot
async function refresh() {
  await render(await invoke('snapshot'));
}

window.addEventListener('DOMContentLoaded', async () => {
  for (const e of document.querySelectorAll('[data-t]')) {
    const english = e.dataset.t;
    e.textContent = await T(english);
    if (e.tagName === 'BUTTON') tipFor(e, english);
  }
  for (const [id, english] of [['powerBtn', 'Stop'], ['restartBtn', 'Restart tunnel'], ['updateBtn', 'Update']]) tipFor($(id), english);
  $('checkBtn').onclick = () => act('check');
  $('restartBtn').onclick = () => act(snap.mode.ronly ? 'test_router' : 'restart_pc');
  $('powerBtn').onclick = power;
  $('cancelBtn').onclick = () => invoke('cancel');
  $('updateBtn').onclick = notYet;
  $('logBtn').onclick = async () => {
    logShown = !logShown;
    $('logBox').hidden = !logShown;
    $('logBtn').textContent = await T(logShown ? 'Hide' : 'Show');
  };
  const auto = () => invoke('set_auto', { on: $('autoOn').checked, fix: $('autoFix').checked, minutes: Number($('autoMin').value) || null });
  $('autoOn').onchange = auto;
  $('autoFix').onchange = auto;
  $('autoMin').onchange = auto;
  $('bgOn').onchange = () => act('set_startup', { on: $('bgOn').checked });
  $('bootOn').onchange = () => act('set_boot', { on: $('bootOn').checked });
  document.addEventListener('click', e => {   // a click anywhere else clears the selection and the menu (click_off)
    $('menu').hidden = true;
    if (!e.target.closest('.nav') && sel) { sel = null; drawLists(); }
  });
  window.addEventListener('resize', () => snap && drawHistory());
  await listen('log', e => logLine(e.payload.at, e.payload.line));
  await listen('changed', refresh);
  await listen('tray', refresh);
  await refresh();
  act('check');
});
