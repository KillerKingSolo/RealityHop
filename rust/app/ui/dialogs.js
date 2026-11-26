// The window's dialogs: node details and Settings (NodeDetails, SettingsDialog). They show inside the window.
'use strict';

// Show a dialog's content; null closes it and runs what the dialog asked for on close
let panelClosed = null;
function panel(content) {
  if (!content && panelClosed) { const f = panelClosed; panelClosed = null; f(); }
  $('panelBox').replaceChildren(...(content ? [progSlot(), ...content] : []));
  $('panel').hidden = !content;
}

// A labelled details grid
function gridRows(rows) {
  const g = el('div', 'grid');
  for (const [k, v] of rows) g.append(el('div', 'k', k), el('div', 'v', v));
  return g;
}

// One node: where it is, how it tests, and its tunnel ping over the last 24 hours (NodeDetails)
async function details(key, tag) {
  const d = await invoke('node_details', { key, tag });
  const card = el('div', 'card');
  card.append(gridRows(d.rows));
  const label = el('div', 'label', await T('TUNNEL PING, LAST 24 HOURS'));
  label.style.margin = '16px 0 4px';
  const chart = el('div', 'card');
  const cv = el('canvas', 'ping');
  chart.append(cv);
  const foot = el('div', 'foot-buttons');
  const close = el('button', '', await T('Close'));
  close.onclick = () => panel(null);
  foot.append(close);
  panel([el('div', 'title', d.title), el('div', 'c-' + d.state, d.sentence), card, label, chart, foot]);
  await drawPings(cv, d.pings, d.slow);
}

// The tunnel ping chart of one node (NodeDetails.draw)
async function drawPings(cv, pts, slow) {
  const w = cv.clientWidth, h = cv.clientHeight, dpr = devicePixelRatio || 1, font = getComputedStyle(document.body).fontFamily;
  cv.width = w * dpr; cv.height = h * dpr;
  const g = cv.getContext('2d');
  g.scale(dpr, dpr);
  if (!pts.length) {
    g.fillStyle = '#8d8d97'; g.textAlign = 'center'; g.font = '10pt ' + font;
    g.fillText(await T('No history yet. It fills in with every hourly check.'), w / 2, h / 2);
    return;
  }
  g.font = '7pt ' + font;
  const now = Date.now() / 1000, top = Math.max(slow, ...pts.filter(p => p[1]).map(p => p[1])) * 1.15;
  const y = v => h - 18 - v / top * (h - 30), x = t => 36 + (1 - (now - t) / 86400) * (w - 46);
  g.textAlign = 'right';
  for (const v of [0, slow]) {
    g.strokeStyle = '#26262d'; g.setLineDash(v ? [2, 3] : []);
    g.beginPath(); g.moveTo(36, y(v)); g.lineTo(w - 10, y(v)); g.stroke();
    g.fillStyle = '#5c5c66'; g.fillText(String(v), 32, y(v) + 3);
  }
  g.setLineDash([]);
  const good = pts.filter(p => p[1] !== null);
  if (good.length > 1) {
    g.strokeStyle = '#5fd38d'; g.lineWidth = 2; g.beginPath();
    good.forEach((p, i) => i ? g.lineTo(x(p[0]), y(p[1])) : g.moveTo(x(p[0]), y(p[1])));
    g.stroke(); g.lineWidth = 1;
  }
  g.textAlign = 'center';
  for (const [t, v] of pts) {
    if (v === null) { g.fillStyle = '#ff4d4f'; g.font = '9pt ' + font; g.fillText('✕', x(t), h - 6); continue; }
    g.fillStyle = v > slow ? '#f5a524' : '#5fd38d';
    g.beginPath(); g.arc(x(t), y(v), 3, 0, 2 * Math.PI); g.fill();
  }
}

// A toggle with its text; the toggle and its checkbox
function toggleRow(text, on) {
  const l = el('label', 'toggle');
  const i = el('input'); i.type = 'checkbox'; i.checked = !!on;
  l.append(i, el('span', 'sw'), el('span', '', text));
  return [l, i];
}

// Compare two version texts by their first three numbers: 1.12.0 > 1.11.9
function cmpVer(a, b) {
  const p = x => (x.match(/\d+/g) || []).slice(0, 3).map(Number);
  const [x, y] = [p(a), p(b)];
  for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) - (y[i] || 0);
  return 0;
}

// Settings (SettingsDialog)
async function openSettings() {
  const s = await invoke('settings_get'), v = s.values;
  const FIELDS = [['check_min', 'Check the nodes every (minutes)', 15, 1440, 15],
    ['slow_ms', 'A working node counts as slow above (ms)', 50, 5000, 25],
    ['tolerance_ms', 'Switch to a faster node when it is faster by (ms)', 0, 1000, 5],
    ['top', 'Candidates tested per scan', 10, 120, 10],
    ['speed_hours', 'Check the node speeds every (hours; each check can use up to 50 MB per node)', 1, 24, 1]];
  const nums = {}, form = el('div', 'form');
  for (const [k, label, lo, hi, step] of FIELDS) {
    const i = el('input'); i.type = 'number'; i.min = lo; i.max = hi; i.step = step; i.value = v[k];
    nums[k] = i;
    form.append(el('label', '', await T(label)), i);
  }
  const c1 = el('div', 'card'); c1.append(form);
  const lang = el('select');
  for (const [code, name] of s.langs) { const o = el('option', '', name); o.value = code; o.selected = code === s.lang; lang.append(o); }
  const langRow = el('div', 'btnrow'); langRow.style.alignItems = 'center'; langRow.style.marginTop = '12px';
  langRow.append(el('span', '', await T('Language')), lang);
  const c2 = el('div', 'card'), toggles = {};
  const items = [['router', 'Router features (GL.iNet): show the router section', v.router],
    ['ronly', 'Router only: no tunnel on this PC; the router tunnels your devices', s.ronly],
    ['auto_update', 'Install Reality Hop updates automatically', v.auto_update],
    ['backend_files', 'Show backend files: buttons for the PC, Router and logs folders', v.backend_files]];
  if (!s.mac) items.push(['start_menu', 'Show Reality Hop in the Start menu and Windows search', v.start_menu], ['desktop_icon', 'Desktop icon', v.desktop_icon]);
  for (const [k, text, on] of items) { const [row, input] = toggleRow(await T(text), on); toggles[k] = input; c2.append(row); }
  // sing-box channels: each choice shows the version it gives and marks the one running
  const sb = el('div', 'form'); sb.style.marginTop = '12px';
  const names = { '': await T('Same as this PC'), stable: await T('Stable'), testing: await T('Test builds (beta, alpha)') };
  const installed = await T('Installed');
  let pcSel = null, rcSel = null, vers = { pc: {}, router: {} };
  const note = el('div', 'note');
  const fill = async () => {
    const opt = (k, ver, now) => names[k] + (ver ? '  ·  ' + ver + (ver === now ? '  (' + installed + ')' : '') : '');
    const change = async (what, old, nw) => !old || !nw || old === nw ? '' :
      T('{what}: {old} → {new} ({how})', { what, old, new: nw, how: await T(cmpVer(nw, old) > 0 ? 'upgrade' : 'downgrade') });
    const notes = [];
    if (pcSel) {
      for (const o of pcSel.options) o.textContent = opt(o.value, vers.pc[o.value], s.pc_now);
      notes.push(await change(await T("PC's sing-box"), s.pc_now, vers.pc[pcSel.value]));
    }
    if (rcSel) {
      const pcCh = (pcSel ? pcSel.value : v.singbox) === 'testing' ? 'preview' : 'stable';
      const ch = k => k === '' ? pcCh : k === 'testing' ? 'preview' : 'stable';
      const rc = s.rc_now || {}, ours = rc.mark ? rc.ver : null;
      for (const o of rcSel.options) o.textContent = opt(o.value, vers.router[ch(o.value)], ours);
      notes.push(await change(await T("Router's sing-box"), rc.ver, vers.router[ch(rcSel.value)]));
    }
    const n = notes.filter(Boolean);
    note.textContent = n.length ? [await T('When you save:'), ...n].join('\n') : '';
  };
  const select = (keys, now) => {
    const x = el('select');
    for (const k of keys) { const o = el('option', '', names[k]); o.value = k; o.selected = k === now; x.append(o); }
    x.onchange = fill;
    return x;
  };
  if (!s.ronly) { pcSel = select(['stable', 'testing'], v.singbox); sb.append(el('label', '', await T("PC's sing-box")), pcSel); }
  if (s.router) { rcSel = select(['', 'stable', 'testing'], v.router_singbox); sb.append(el('label', '', await T("Router's sing-box")), rcSel); }
  c2.append(sb, note);
  invoke('sb_versions').then(x => { if (x) { vers = x; fill(); } });
  fill();
  const links = el('div', 'links');
  for (const [text, url] of [[`github.com/${s.repo}`, `https://github.com/${s.repo}`],
    ["What's new (changelog)", `https://github.com/${s.repo}/blob/main/CHANGELOG.md`],
    ['Nodes come from github.com/rtwo2/FastNodes', 'https://github.com/rtwo2/FastNodes'],
    ['Tunnel engine: github.com/SagerNet/sing-box', 'https://github.com/SagerNet/sing-box']]) {
    const a = el('a', '', await T(text)); a.onclick = () => invoke('open_url', { url }); links.append(a);
  }
  const updNote = el('div', 'note'), row = el('div', 'btnrow');
  const button = async (english, fn) => { const b = el('button', 'outline', await T(english)); b.onclick = fn; tipFor(b, english); row.append(b); };
  let gen = 0;
  await button('Check for updates', async () => {
    const mine = ++gen;
    updNote.className = 'note'; updNote.textContent = await T('Checking') + '...';
    try {
      const nv = await invoke('check_updates');
      if (mine !== gen || !updNote.isConnected) return;   // Settings closed while it checked
      // an update: close Settings and show the update window
      if (nv) { panel(null); return openUpdate(); }
      updNote.textContent = await T('Reality Hop {ver} is the newest version.', { ver: snap.release }) + '  ' + new Date().toLocaleTimeString();
    } catch (e) {
      if (mine !== gen) return;
      updNote.className = 'note c-bad'; updNote.textContent = await T('Could not reach GitHub: {e}', { e: String(e) });
    }
  });
  await button('Run setup again', () => { panel(null); rerunSetup(); });
  if (s.router) await button('Change router', changeRouter);
  await button('Uninstall', uninstall);
  c2.append(el('div', 'note', s.version), links, row, updNote);
  const foot = el('div', 'foot-buttons');
  const defaults = el('button', '', await T('Defaults'));
  defaults.onclick = () => { for (const [k] of FIELDS) nums[k].value = s.defaults[k]; };
  const cancel = el('button', '', await T('Cancel'));
  cancel.onclick = () => panel(null);
  const save = el('button', 'accent', await T('Save'));
  save.onclick = async () => {
    const changes = {};
    for (const [k] of FIELDS) {
      if (!/^\d+$/.test(nums[k].value)) return info(await T('Numbers only, please.'));
      changes[k] = Number(nums[k].value);
    }
    for (const k of ['router', 'auto_update', 'backend_files', 'start_menu', 'desktop_icon']) if (toggles[k]) changes[k] = toggles[k].checked ? 1 : 0;
    if (pcSel) changes.singbox = pcSel.value;
    if (rcSel) changes.router_singbox = rcSel.value;
    changes.lang = lang.value;
    const r = await invoke('settings_save', { changes, routerOnly: toggles.ronly.checked });
    panel(null);
    for (const m of r.messages) await info(m);
    if (r.ask_router && await yesNo('Router features', await T('The router is not set up yet. Open the router steps of the setup wizard now?'))) {
      location.href = 'setup.html#ronly'; return;
    }
    if (r.relayout) location.reload();
  };
  foot.append(defaults, cancel, save);
  const margin = el('div', 'note', await T('The switching margin takes effect on the next tunnel restart or router push.'));
  panel([el('div', 'title', await T('Settings')), c1, margin, langRow, c2, foot]);
}

// Open the setup wizard again; the tunnel keeps running (rerun_setup)
async function rerunSetup() {
  if (await yesNo('Run setup again', await T('Open the setup wizard? This window closes; the tunnel keeps running.\n\nNodes already in use are kept. Use it to add a GL.iNet router, redo the VPN step or the background start.')))
    location.href = 'setup.html';
}

// A new or reset router: forget the old one, then the router steps of the setup (change_router)
async function changeRouter() {
  if (!await yesNo('Change router', await T('For a new router, a reset router or a new router address.\n\nReality Hop forgets the old router and opens the router steps of the setup, where you connect the new one. This window closes; the PC tunnel keeps running.'))) return;
  await invoke('router_forget');
  location.href = 'setup.html#ronly';
}

// Remove Reality Hop from this PC, after three questions (SettingsDialog.uninstall)
async function uninstall() {
  const t = await T('Uninstall Reality Hop'), yn = [['No', false], ['Yes', true, true]];
  const what = snap.mode.mac ? 'Remove Reality Hop from this Mac?' : 'Remove Reality Hop from this PC?\n\nThe tunnel, the background watcher, the boot start (one UAC prompt), the shortcuts and the program are removed.';
  if (!await ask(t, await T(what), yn)) return;
  const keep = await ask(t, await T('Keep your settings and node vault for a later install?'), yn);
  const router = snap.mode.router && await ask(t, await T('Also switch the tunnel on the router off?'), yn);
  try {
    const out = await invoke('uninstall', { keep, router });
    await info(await T('Reality Hop is removed.\n\n') + out.join('\n'));
    invoke('quit');
  } catch (e) {
    info(String(e));
  }
}

window.addEventListener('DOMContentLoaded', () => {
  $('settingsBtn').onclick = openSettings;
  $('panel').addEventListener('mousedown', e => { if (e.target === $('panel')) panel(null); });
  document.addEventListener('keydown', e => { if (e.key === 'Escape') { panel(null); $('menu').hidden = true; } });
});

// Report a problem: a redacted copy of the logs plus a short summary, shown before anything leaves this PC (ReportDialog)
async function openReport() {
  const head = el('div', 'muted', await T('This makes a redacted report from the three logs (window, watch, core) and a short summary of your setup. Addresses, MAC addresses, your user and computer name, folder paths, e-mail addresses, keys and secrets are redacted. Nothing is sent: read it, then attach the file to a GitHub issue.'));
  head.style.marginTop = '8px';
  const card = el('div', 'card report');
  const pre = el('pre', '', await T('Making the report...'));
  card.append(pre);
  const title = el('div', 'title', await T('Report a problem'));
  const row = el('div', 'foot-buttons'), btns = [];
  let made = null;
  const button = async (english, fn, accent) => {
    const b = el('button', accent ? 'accent' : '', await T(english));
    b.onclick = fn; b.disabled = true; btns.push(b); row.append(b);
  };
  await button('Open GitHub issue', () => invoke('report_issue', { name: made.name }).catch(e => info(String(e))), true);
  await button('Open folder', () => invoke('open_folder', { which: 'reports' }));
  await button('Copy', () => navigator.clipboard.writeText(made.text));
  row.append(el('span', 'grow'));
  const close = el('button', '', await T('Close'));
  close.onclick = () => panel(null);
  row.append(close);
  $('panelBox').classList.add('xwide');
  panelClosed = () => $('panelBox').classList.remove('xwide');
  panel([title, head, card, row]);
  try {
    made = await invoke('report_make');
    pre.textContent = made.text;
    title.textContent = await T('Report a problem') + '  ·  ' + made.name;
    for (const b of btns) b.disabled = false;
  } catch (e) {
    pre.textContent = await T('Could not make the report: {e}', { e: String(e) });
  }
}
