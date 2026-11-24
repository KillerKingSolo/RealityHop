// The Candidates dialog: node-list candidates that could replace yours. Click one, Ctrl+click more: the click order
// numbers them vless-1, vless-2, ... (Candidates)
'use strict';
const CAND_HEAD = [['num', ''], ['server', 'SERVER'], ['front', 'FRONT'], ['where', 'LOCATION'], ['host', 'HOST'],
  ['tcp', '443 PING'],
  ['tunnel', 'TUNNEL'], ['result', 'STATUS']];
let cand = null;   // { view, order, ctx, table, status, pick } while the dialog is open

async function openCandidates() {
  if (cand) return;
  cand = { view: null, order: [], ctx: null };
  const head = el('div', 'muted', await T('Nodes from the public node list that could replace yours, fastest 443 first. Click one, Ctrl+click more: the click order numbers them vless-1, vless-2, ... For one specific slot, click a node and then a vless button below. A 443 answer proves nothing; only a test shows which ones actually work.'));
  head.style.marginTop = '8px';
  const card = el('div', 'card cand-list');
  cand.table = el('table');
  card.append(cand.table);
  const bar1 = el('div', 'btnrow'), bar2 = el('div', 'btnrow'), bar3 = el('div', 'btnrow');
  bar1.style.marginTop = '12px';
  const button = async (bar, english, fn, accent) => {
    const b = el('button', accent ? 'accent' : '', await T(english));
    b.onclick = fn; tipFor(b, english); bar.append(b);
  };
  await button(bar1, 'Ping and test again', () => act('cand_scan'));
  bar1.append(el('span', 'grow'));
  cand.pick = el('span', 'muted');
  bar1.append(cand.pick);
  const m = snap.mode;
  await button(bar2, 'Replace all 6 with the best 6', putBest, true);
  await button(bar2, 'Put picked in slots', putAll, true);
  if (!m.ronly) {
    await button(bar2, 'Restart PC tunnel', () => act('restart_pc'));
    await button(bar2, 'Test PC tunnel', () => act('cand_test_tunnel'));
  }
  if (m.router) await button(bar2, 'Push to router', pushRouter);
  bar3.append(el('span', 'muted', await T('Put the selected node into:')));
  for (let i = 0; i < 6; i++) await button(bar3, `vless-${i + 1}`, () => putSlot(i));
  const strip = el('div', 'card strip');
  const sbar = el('div', 'bar');
  sbar.style.background = 'var(--acc)';
  cand.status = el('div');
  strip.append(sbar, cand.status);
  strip.style.marginTop = '12px';
  const foot = el('div', 'foot-buttons');
  const close = el('button', '', await T('Close'));
  close.onclick = () => panel(null);
  foot.append(close);
  $('panelBox').classList.add('xwide');
  panelClosed = () => { cand = null; $('panelBox').classList.remove('xwide'); invoke('cand_close'); };
  panel([el('div', 'title', await T('Candidates')), head, card, bar1, bar2, bar3, strip, foot]);
  await invoke('cand_open');
}

// Draw the rows from a fresh view (render)
async function drawCandidates() {
  if (!cand) return;
  const v = await invoke('cand_view');
  if (!cand) return;
  if (!v.open) { panel(null); return; }   // the nodes went in: the dialog closes
  cand.view = v;
  cand.order = cand.order.filter(s => v.rows.some(r => r.server === s));
  cand.status.textContent = v.msg;
  const t = cand.table;
  const heads = await Promise.all(CAND_HEAD.map(([, h]) => h ? T(h) : ''));
  const cols = el('colgroup');
  for (const [k] of CAND_HEAD) cols.append(el('col', 'k-' + k));
  const thead = el('thead'), hr = el('tr');
  for (const x of heads) hr.append(el('th', '', x));
  thead.append(hr);
  const body = el('tbody');
  for (const r of v.rows) {
    const n = cand.order.indexOf(r.server);
    const tr = el('tr', r.state + (n >= 0 ? ' sel' : ''));
    for (const x of [n >= 0 ? String(n + 1) : '', r.server, r.front, r.where, r.host, r.tcp, r.tunnel, r.result]) tr.append(el('td', '', x));
    tr.onclick = e => { e.stopPropagation(); pick(r.server, e.ctrlKey || e.metaKey); };
    tr.oncontextmenu = e => { e.preventDefault(); e.stopPropagation(); cand.ctx = r.server; candMenu(e); };
    body.append(tr);
  }
  t.replaceChildren(cols, thead, body);
  await candFit();
  cand.pick.textContent = cand.order.length ? cand.order.map((s, i) => `vless-${i + 1} ← ${s}`).join('   ') : await T('No candidates picked');
}

// A click picks one; Ctrl+click adds or removes, up to the slot count (on_select)
function pick(server, add) {
  const i = cand.order.indexOf(server);
  if (!add) cand.order = [server];
  else if (i >= 0) cand.order.splice(i, 1);
  else if (cand.order.length < cand.view.slots) cand.order.push(server);
  drawCandidates();
}

// The right-click menu: put this one into a slot
async function candMenu(e) {
  const menu = $('menu');
  menu.replaceChildren();
  for (let i = 0; i < 6; i++) {
    const d = el('div', '', await T('Put this one into vless-{n} and restart', { n: i + 1 }));
    d.onclick = () => { menu.hidden = true; putOne(i, cand.ctx); };
    menu.append(d);
  }
  menu.hidden = false;
  menu.style.left = Math.min(e.clientX, innerWidth - menu.offsetWidth - 4) + 'px';
  menu.style.top = Math.min(e.clientY, innerHeight - menu.offsetHeight - 4) + 'px';
}

// The picks as lines for a question (picks)
async function picksText(pairs) {
  const ok = await T('Tested, works'), no = await T('NOT tunnel-tested');
  return pairs.map(([i, s]) => {
    const r = cand.view.rows.find(x => x.server === s);
    return `vless-${i + 1}  ←  ${s}  (${r && r.ok ? ok : no})`;
  }).join('\n');
}

async function putAll() {
  if (!cand.order.length)
    return info(await T('Pick candidates first: click one, Ctrl+click more. The order gives the slot numbers.'));
  const pairs = cand.order.map((s, i) => [i, s]);
  if (await yesNo('Put in slots', await T('Put these into the slots, restart the tunnel and check them?\n\n') + await picksText(pairs)))
    act('cand_put', { picks: pairs });
}

async function putSlot(slot) {
  if (cand.order.length !== 1)
    return info(await T('Select one node in the list (a single click), then click the vless slot it goes into.'));
  putOne(slot, cand.order[0]);
}

async function putOne(slot, server) {
  if (!server) return;
  if (await yesNo('Put in slot', await T('Put this into the slot, restart the tunnel and check it?\n\n') + await picksText([[slot, server]])))
    act('cand_put', { picks: [[slot, server]] });
}

// Only tunnel-tested working nodes go in; it tests first when too few passed (put_best)
async function putBest() {
  const v = cand.view;
  if (!v || !v.rows.length) return info(await T('No candidates yet; ping first.'));
  const k = v.slots, ok = v.rows.filter(r => r.ok).length;
  const first = ok >= k ? '' : await T('It first tests the {top} fastest (about 20 seconds).', { top: await invoke('top') }) + ' ';
  const then = await T(v.pc_only ? 'Then it restarts the PC tunnel and checks every slot.' : v.ronly ?
    'Then it pushes to the router and checks every slot.' : 'Then it pushes to the router, restarts the PC tunnel and checks every slot.');
  if (await yesNo('Replace all', await T('Put the best {k} tested nodes into your slots?', { k }) + ' ' + first +
    await T('Only nodes that pass the test go in.') + ' ' + then))
    act('cand_best');
}

window.addEventListener('DOMContentLoaded', () => listen('changed', drawCandidates));

// The Candidates table's column widths, as on the main page; STATUS keeps room for a working candidate's result
async function candFit() {
  if (!cand || !cand.table) return;
  fitTables([cand.table], CAND_HEAD, await T('Works, {ms} ms in the test, exit {ip}', { ms: '9999', ip: '255.255.255.255' }));
}
window.addEventListener('resize', () => { candFit(); });
