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
  'Ping and test again': 'Downloads the node list again, pings port 443 of each candidate from this PC, then tests the fastest with a separate copy of sing-box: a real handshake and download through each. Runs on its own when this window opens.',
  'Test 50 more': 'Tests the next 50 fastest candidates that are not tested yet, even when 6 already work. Use it to find more working nodes to pick from.',
  'Test all': 'Tests every candidate, 50 at a time, including the ones that failed the last scan. It can take several minutes; Cancel stops it.',
  'Replace all 6 with the best 6': 'Puts the 6 fastest tunnel-tested candidates into your slots (it tests first if too few passed), restarts the tunnel, checks every slot and closes this window.',
  'Put picked in slots': 'Puts the candidates you picked into the slots, in the order you clicked them, restarts the tunnel and checks them.',
  'Test PC tunnel': 'Tests the nodes in your slots through the running PC tunnel, as Check now does. Restart first after a change.',
  'drag-pin': 'Drag onto another row: PC, Router, or the & for both. That node is used until it fails, then the pick is automatic again.',
  'Show candidates': 'Opens the nodes from the public node list that could replace yours. Ping and test them, then put them in slots.',
  'Fix: test online nodes': 'Replaces the selected node with the best node from the online list that passes a test.',
  'Fix: nodes that worked before': 'Replaces the selected node with the best node from the vault of nodes that worked before.',
  'Fix: quick swap, untested': 'Replaces the selected node at once with the next fastest node, without a test.',
  'Replace all with best 6': 'Tests the fastest nodes from the online list and your current ones, then puts the best 6 in. About a minute.',
  'Rescue from verified vault': 'For when every node is dead. Tests the vault of nodes that worked before and puts the best 6 in.',
  'Restart PC tunnel': 'Restarts the PC tunnel on the nodes in your slots. About 2 seconds.',
  'Change router': 'For a new or reset router: forgets the old one and opens the router steps of the setup.',
  'Install update': 'Installs the new release now. The window restarts; the tunnel keeps running.',
  'Later': 'Closes this window. The Update button at the top stays until you update.',
  'Add': 'Adds the address you typed to the list.',
  'Refresh list': "Reads the router's list of devices again.",
  'Save and push': 'Saves the device list and sends it to the router.',
  'Open GitHub issue': 'Opens a new GitHub issue with the summary filled in. Drag the report file into it.',
  'Open folder': 'Opens the folder with the reports.',
  'Copy': 'Copies the report to the clipboard.',
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
const HEAD = [['slot', 'SLOT'], ['server', 'SERVER'], ['front', 'FRONT'], ['where', 'LOCATION'], ['host', 'HOST'],
  ['tcp', '443 PING'],
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
let tipJob = null, histPts = [];
function tipFor(e, english) {
  e.addEventListener('mouseenter', () => {
    clearTimeout(tipJob);
    const slot = /^vless-(\d)$/.exec(english);
    if (!TIPS[english] && !slot) return;
    tipJob = setTimeout(async () => {
      const t = $('tip'), r = e.getBoundingClientRect();
      t.textContent = slot ? await T('Puts the selected candidate into slot vless-{n}, restarts the tunnel and checks it.', { n: slot[1] }) : await T(TIPS[english]);
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

// Sidebar group icons: the Windows icon font, and SVG copies of the same shapes on macOS, which lacks that font
const MAC = /Mac/.test(navigator.userAgent);
const svg = body => `<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
const ICONS = {
  nodes: ['', svg('<circle cx="8" cy="8" r="6.5"/><ellipse cx="8" cy="8" rx="2.8" ry="6.5"/><path d="M1.9 5.6h12.2M1.9 10.4h12.2"/>')],
  pc: ['', svg('<path d="M7.5 5.5V2.5a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1V13a1 1 0 0 0 1 1H6M3.3 3.7h2.2M3.3 5.5h1.2"/><rect x="6.5" y="6" width="8" height="6" rx="1"/><path d="M10.5 12v2M8.8 14.2h3.4"/>')],
  router: ['', svg('<path d="M2.7 7.2a5.3 5.3 0 0 1 10.6 0M5.2 7.2a2.8 2.8 0 0 1 5.6 0"/><circle cx="8" cy="7" r="1.4"/><path d="M8 8.4v2.1"/><rect x="2.8" y="10.5" width="10.4" height="3.6" rx="1.8"/>')],
  help: ['', svg('<path d="M5.6 5.4a2.4 2.4 0 1 1 3.5 2.1c-.7.4-1.1.9-1.1 1.7v.9"/><circle cx="8" cy="13" r=".55" fill="currentColor" stroke="none"/>')],
};

// ---- sidebar: every action, grouped
async function buildSide(m) {
  const key = JSON.stringify(m);
  if (built === key) return;
  built = key;
  const nav = $('groups');
  nav.replaceChildren();
  const group = async (title, icon) => {
    const g = el('div', 'group');
    const i = el('span', 'gi');
    if (MAC) i.innerHTML = ICONS[icon][1]; else i.textContent = ICONS[icon][0];   // fixed strings above
    g.append(i, el('span', 'gt', await T(title)));
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
  await group('Nodes', 'nodes');
  await item('Show candidates', openCandidates);
  await item('Fix: test online nodes', () => fixSelected('online'));
  await item('Fix: nodes that worked before', () => fixSelected('vault'));
  await item('Fix: quick swap, untested', () => fixSelected('quick'));
  await item('Replace all with best 6', replaceAll);
  await item('Rescue from verified vault', rescue);
  if (!m.ronly) {
    await group('PC tunnel', 'pc');
    await item('Restart PC tunnel', () => act('restart_pc'));
    await item('Run setup again', rerunSetup);
  }
  if (m.router) {
    await group('Router', 'router');
    await item('Test on router', () => act('test_router'));
    await item('Devices', openDevices);
    await item('Push to router', pushRouter);
    if (m.ronly) await item('Run setup again', rerunSetup);
  }
  await group(m.backend ? 'Files & help' : 'Help', 'help');
  await item('Report a problem', openReport);
  if (m.backend) {
    await item('Open backend folder', () => invoke('open_folder', { which: 'data' }));
    await item('Open logs folder', () => invoke('open_folder', { which: 'logs' }));
  }
  $('bootRow').hidden = m.ronly;
  $('bootText').textContent = await T(m.mac ? 'Start tunnel at login' : 'Start tunnel at boot');
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
  $('modeLine').textContent = (await T(m.ronly ? 'Router only' : m.router ? 'PC + Router' : 'PC only')) + '  ·  ' +
    (s.core_mode || m.ronly ? s.version : await T('rh-core MISSING'));
  $('modeLine').classList.toggle('beta', / beta /.test(s.version));   // a test build stands out
  $('coreLine').textContent = s.core || '';
  $('rcoreLine').textContent = [s.rcore, s.router_away ? await T('Router not connected') : ''].filter(Boolean).join('\n');
  $('rcoreLine').classList.toggle('away', !!s.router_away);
  $('autoOn').checked = s.auto_on;
  $('autoFix').checked = s.auto_fix;
  if (document.activeElement !== $('autoMin')) $('autoMin').value = s.check_min;
  $('bgOn').checked = s.startup_on && !s.stopped;
  $('bootOn').checked = s.boot_on;
  $('nextAuto').textContent = s.next_auto;
  // header
  // the sidebar dot shows the tray's state, as the window icon does
  document.querySelector('.brand .dot').className = 'dot c-' + ((s.tray && s.tray.state) || 'unk');
  const pill = $('pill');
  pill.className = 'pill ' + s.status.state;
  pill.textContent = s.status.pill;
  drawProg(s.busy, s.prog);
  $('cancelBtn').disabled = !s.busy;
  $('powerBtn').textContent = await T(s.stopped ? 'Start' : 'Stop');
  $('restartBtn').textContent = await T(m.ronly ? 'Test on router' : 'Restart tunnel');
  $('updateBtn').hidden = !s.update;
  if (!s.busy) updQueued = false;
  if (s.update) $('updateBtn').textContent = updQueued ? await T('Update queued') : await T('Update to {ver}', { ver: s.update });
  // tiles
  const tiles = $('tiles');
  tiles.style.gridTemplateColumns = `repeat(${s.tiles.length}, minmax(0, 1fr))`;
  tiles.replaceChildren(...s.tiles.map(t => {
    const d = el('div', 'tile' + (t.act ? ' click' : ''));
    // the router tiles' line says what to press; the PC tile's line shows the node and exit
    const sub = el('div', 'ts' + (t.act && t.act !== 'restart_pc' ? ' link' : ''), t.sub);
    if (t.act) d.onclick = () => t.act === 'push_router' ? pushRouter() : act(t.act);
    if (t.act === 'restart_pc') tipFor(d, 'Restart PC tunnel');
    d.append(el('div', 'tl', t.label), el('div', 'tv c-' + t.color, t.value), sub);
    return d;
  }));
  // status line
  // a problem line opens the activity log; "logs/core.log" in it opens that file
  const st = $('status'), bad = s.status.state === 'bad';
  st.replaceChildren(...s.status.text.split(/(logs\/core\.log)/).map(p => {
    if (p !== 'logs/core.log') return document.createTextNode(p);
    const a = el('span', 'link', p);
    a.onclick = e => { e.stopPropagation(); invoke('open_folder', { which: 'core_log' }); };
    return a;
  }));
  st.className = (s.status.state === 'ok' || s.status.state === 'unk' ? '' : 'c-' + s.status.state) + (bad ? ' click' : '');
  st.onclick = bad ? () => {
    if (!logShown) $('logBtn').click();
    $('log').focus(); $('log').scrollIntoView({ behavior: 'smooth', block: 'start' }); $('log').scrollTop = $('log').scrollHeight;
  } : null;
  $('statusBar').style.background = s.status.state === 'unk' ? 'var(--acc)' : `var(--${s.status.state})`;
  await drawHistory();
  await drawLists();
}

// The node tables (render)
async function drawLists() {
  const box = $('lists');
  const heads = await Promise.all(HEAD.map(([, h]) => T(h)));
  const using = { split: (await T('In use by {who}', { who: '\u0001' })).split('\u0001'), pc: await T('PC'), router: await T('Router') };
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
      for (const v of [r.tag, r.server, r.front, r.where, r.host, r.tcp, r.tunnel]) tr.append(el('td', '', v));
      tr.append(statusCell(r, using));
      tr.dataset.server = r.server;
      tr.onclick = e => { e.stopPropagation(); sel = { key: l.key, tag: r.tag }; drawLists(); };
      tr.oncontextmenu = e => { e.preventDefault(); e.stopPropagation(); sel = { key: l.key, tag: r.tag }; drawLists(); popup(e); };
      tr.ondblclick = () => details(l.key, r.tag);
      body.append(tr);
    }
    t.append(cols, thead, body);
    c.append(h, t);
    return c;
  }));
  await fitCols();
}

// A row's STATUS: "In use by PC & Router" with PC, & (both) and Router as handles to drag onto another row
function statusCell(r, using) {
  const td = el('td', '', r.lead);
  if (!r.pc && !r.router) return td;
  td.append('   ·   ' + using.split[0]);
  const grab = (text, who) => {
    const s = el('span', 'grab', text);
    s.onmousedown = e => { if (e.button === 0) { e.preventDefault(); e.stopPropagation(); pinDrag(e, who, who === 'both' ? using.pc + ' & ' + using.router : text); } };
    tipFor(s, 'drag-pin');
    return s;
  };
  if (r.pc) td.append(grab(using.pc, 'pc'));
  if (r.pc && r.router) td.append(grab(' & ', 'both'));
  if (r.router) td.append(grab(using.router, 'router'));
  td.append(using.split[1] || '');
  return td;
}

// Drag PC, Router or both onto a row: that node is used until it fails (pin). Mouse events, not HTML drag and drop:
// the window takes drops for itself on Windows
function pinDrag(e, who, label) {
  const ghost = el('div', 'pin-ghost', label);
  document.body.append(ghost);
  let over = null;
  const move = m => {
    ghost.style.left = m.clientX + 12 + 'px'; ghost.style.top = m.clientY + 8 + 'px';
    const row = document.elementFromPoint(m.clientX, m.clientY)?.closest('#lists tbody tr');
    if (row !== over) { over?.classList.remove('drop'); over = row; over?.classList.add('drop'); }
  };
  const up = async () => {
    removeEventListener('mousemove', move); removeEventListener('mouseup', up);
    ghost.remove(); over?.classList.remove('drop');
    const server = over?.dataset.server;
    if (!server) return;
    const names = { pc: await T('PC'), router: await T('Router'), both: await T('PC') + ' & ' + await T('Router') };
    if (await yesNo('Use this node', await T('{who} will use {server} until that node fails, then pick automatically again.', { who: names[who], server })))
      act('pin', { who, server });
  };
  addEventListener('mousemove', move); addEventListener('mouseup', up);
  move(e);
}

// The node tables' column widths (fitTables)
async function fitCols() {
  const sample = '●  ' + await T('Working') + '   ·   ' + await T('In use by {who}', { who: await T('PC') + ' & ' + await T('Router') });
  fitTables([...$('lists').querySelectorAll('table')], HEAD, sample);
}

// Column widths from the longest text in each column, the same in every table given. The last column (STATUS) keeps
// room for sample and a little more; when the window is too narrow the text columns (server, front, location, host)
// shrink first, widest first, down to 70 px (ellipsis), never STATUS
function fitTables(tables, head, sample) {
  const td = tables[0] && tables[0].querySelector('tbody td'), th = tables[0] && tables[0].querySelector('th');
  if (!td || !tables[0].clientWidth) return;
  const g = (fitTables.c ||= document.createElement('canvas')).getContext('2d');
  const width = (s, font) => { g.font = font; return g.measureText(s).width; };
  // the shorthand font can come back empty in WebView2; build it from its parts
  const fontOf = e => { const c = getComputedStyle(e); return `${c.fontStyle} ${c.fontWeight} ${c.fontSize} ${c.fontFamily}`; };
  const tdFont = fontOf(td), thFont = fontOf(th), pad = 16 + 8;
  const n = head.length;
  const want = head.map(() => 0);
  for (const t of tables) {
    t.querySelectorAll('th').forEach((x, i) => { want[i] = Math.max(want[i], width(x.textContent, thFont) + pad); });
    for (const r of t.querySelectorAll('tbody tr'))
      r.querySelectorAll('td').forEach((x, i) => { want[i] = Math.max(want[i], width(x.textContent, tdFont) + pad); });
  }
  const statusMin = width(sample, tdFont) + pad + 24;
  const avail = tables[0].clientWidth;
  const w = want.slice(0, n - 1);
  const soft = ['server', 'front', 'where', 'host'].map(k => head.findIndex(([h]) => h === k)).filter(i => i >= 0);
  let over = w.reduce((a, b) => a + b, 0) + statusMin - avail;
  while (over > 0.5) {
    const i = soft.filter(i => w[i] > 70).sort((a, b) => w[b] - w[a])[0];
    if (i === undefined) break;
    const next = Math.max(70, ...soft.filter(j => j !== i && w[j] < w[i]).map(j => w[j]));
    const cut = Math.min(over, w[i] - next || w[i] - 70);
    w[i] -= cut; over -= cut;
  }
  for (const t of tables) t.querySelectorAll('col').forEach((c, i) => { c.style.width = i < n - 1 ? Math.ceil(w[i]) + 'px' : ''; });
}

// The right-click menu of a node (popup)
async function popup(e) {
  const menu = $('menu');
  menu.replaceChildren();
  const auto = el('div', '', await T('PC and router: pick nodes automatically again'));
  auto.onclick = () => { menu.hidden = true; act('pin', { who: 'both', server: null }); };
  menu.append(auto);
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
  // mark it before the first await: two draws close together must not both fill it
  if (!legend.dataset.done) {
    legend.dataset.done = '1';
    const kids = [];
    for (const k of ['drop', 'back', 'replace', 'restart', 'rescue']) {
      const d = el('span', k === 'back' ? 'd bar' : 'd', k === 'back' ? '|' : '●');
      d.style.color = EVENTS[k][0];
      kids.push(d, el('span', 'n', await T(EVENTS[k][1])));
    }
    legend.replaceChildren(...kids);
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
    g.fillText(hr === 0 ? await T('Now') : await T('-{n}h', { n: hr }), x, h - 1);
  }
  const counts = {};
  histPts = [];
  let dropAt = null;
  for (const [t, kind, text] of snap.events) {
    const col = (EVENTS[kind] || ['#8d8d97'])[0], x = R - (now - t) / span * (R - L), y = kind === 'drop' || kind === 'back' ? 14 : 6;
    histPts.push({ x, y, t, kind, text: text || '', after: kind === 'back' && dropAt !== null ? Math.round(t - dropAt) : null });
    if (kind === 'drop') dropAt = t; else if (kind === 'back') dropAt = null;
    g.strokeStyle = g.fillStyle = col;
    if (kind === 'back') { g.beginPath(); g.moveTo(x, y - 3); g.lineTo(x, y + 3); g.stroke(); }
    else { g.beginPath(); g.arc(x, y, 3, 0, 2 * Math.PI); g.fill(); }
    counts[kind] = (counts[kind] || 0) + 1;
  }
  const parts = [];
  for (const [k, e] of [['PIA Drops {n}', 'drop'], ['Replacements {n}', 'replace'], ['Restarts {n}', 'restart'], ['Rescues {n}', 'rescue']])
    parts.push(await T(k, { n: counts[e] || 0 }));
  const down = snap.down || [];
  if (down.length) parts.push(await T('PIA down {s} s, longest {m} s', { s: Math.round(down.reduce((a, b) => a + b, 0)), m: Math.round(Math.max(...down)) }));
  $('histInfo').textContent = parts.join('   ·   ');
}

// The chart's tooltip: the event nearest the pointer, its time and what happened
async function histTip(e) {
  const t = $('tip'), r = $('hist').getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
  let best = null;
  for (const p of histPts) {
    const d = Math.hypot(p.x - x, p.y - y);
    if (d < 8 && (!best || d < best.d)) best = { ...p, d };
  }
  if (!best) { t.hidden = true; return; }
  const at = new Date(best.t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  let line = at + '  ' + await T((EVENTS[best.kind] || [, best.kind])[1]);
  if (best.after !== null) line += '  ' + await T('after {n} s', { n: best.after });
  else if (best.kind !== 'drop' && best.kind !== 'back' && best.text) line += '\n' + best.text;
  t.textContent = line;
  t.hidden = false;
  t.style.left = Math.min(e.clientX + 12, innerWidth - t.offsetWidth - 8) + 'px';
  t.style.top = (r.bottom + 4 + t.offsetHeight > innerHeight - 8 ? r.top - t.offsetHeight - 4 : r.bottom + 4) + 'px';
}

// One activity line, with its time (drain); Rust sends it with a capital first letter (cap_first)
function logLine(at, line) {
  const log = $('log');
  const end = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
  log.append(el('span', 't', at + '  '), el('span', /^(Error|FAIL)/.test(line) || line.includes('NOT ') ? 'err' : '', line + '\n'));
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
  $('histClear').onclick = async () => {
    if (await yesNo('Clear the chart?', await T('This removes the events of the last 24 hours from the chart and from problem reports. Older events and node pings stay.')))
      await invoke('history_clear');
  };
  // Ctrl+A (Cmd+A) in the activity log selects only the log
  $('log').addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') { e.preventDefault(); getSelection().selectAllChildren($('log')); }
  });
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
  window.addEventListener('resize', () => { if (snap) { drawHistory(); fitCols(); } });
  $('hist').addEventListener('mousemove', histTip);
  $('hist').addEventListener('mouseleave', () => { $('tip').hidden = true; });
  await listen('log', e => logLine(e.payload.at, e.payload.line));
  await listen('changed', refresh);
  await listen('tray', refresh);
  await refresh();
  await fitOnce();
  act('check');
});

// The first draw: the window grows to show everything, with no scroll bar
async function fitOnce() {
  await document.fonts.ready;
  await new Promise(r => setTimeout(r, 50));   // not requestAnimationFrame: it never fires in a window not yet shown
  const extra = Math.max(...['main', 'side'].map(id => $(id).scrollHeight - $(id).clientHeight));
  if (extra > 0) await invoke('fit_height', { extra });
}
