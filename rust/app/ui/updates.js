// A new Reality Hop release (UpdateDialog) and the router build of sing-box (RouterCoreDialog). Rust looks for both
// when the window opens and sends an "offer" event; nothing installs unasked.
'use strict';
let updQueued = false;

// Release notes (Markdown) as [kind, text]: continuation lines joined, the checksum footer left out (md_blocks)
function mdBlocks(text) {
  const blocks = [];
  for (const line of text.replace(/\r/g, '').split('\n')) {
    const s = line.trim();
    if (!s || s === '---') { blocks.push(null); continue; }
    if (s.startsWith('- ') || s.startsWith('* ')) blocks.push(['li', s.slice(2)]);
    else if (blocks.length && blocks[blocks.length - 1]) blocks[blocks.length - 1][1] += ' ' + s;
    else blocks.push(['p', s.replace(/^#+/, '').trim()]);
  }
  return blocks.filter(Boolean);
}

// One line of Markdown as elements: **bold**, `code`, [text](link) shown as its text (md_insert)
function mdLine(parent, text) {
  text = text.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
  for (const part of text.split(/(\*\*[^*]+\*\*|`[^`]+`)/)) {
    if (part.startsWith('**') && part.endsWith('**') && part.length > 4) parent.append(el('b', '', part.slice(2, -2)));
    else if (part.startsWith('`') && part.endsWith('`') && part.length > 2) parent.append(el('code', '', part.slice(1, -1)));
    else if (part) parent.append(document.createTextNode(part));
  }
}

// What changed in every version since this one, then install now or later (UpdateDialog)
async function openUpdate() {
  const u = await invoke('update_info');
  if (!u || !$('panel').hidden) return;
  const notes = el('div', 'card notes');
  for (const [ver, date, text] of u.changes) {
    const h = el('div', 'ver', await T('Version {ver}', { ver }));
    if (date) h.append(el('span', 'date', '   ' + date));
    notes.append(h);
    for (const [kind, t] of mdBlocks(text)) {
      const d = el('div', kind);
      if (kind === 'li') d.append(el('span', 'dot', '•  '));
      mdLine(d, t);
      notes.append(d);
    }
  }
  const row = el('div', 'foot-buttons');
  const install = el('button', 'accent', await T('Install update'));
  install.onclick = async () => {
    panel(null);
    if (snap.busy) { updQueued = true; $('updateBtn').textContent = await T('Update queued'); }
    act('app_update');
  };
  const later = el('button', '', await T('Later'));
  later.onclick = () => panel(null);
  tipFor(install, 'Install update'); tipFor(later, 'Later');
  const links = el('div', 'links');
  const a = el('a', '', await T('All changes on GitHub'));
  a.onclick = () => invoke('open_url', { url: `https://github.com/${u.repo}/blob/main/CHANGELOG.md` });
  links.append(a);
  row.append(install, later, el('span', 'grow'), links);
  panel([el('div', 'title', await T('Reality Hop {ver} is ready', { ver: u.ver })),
    el('div', 'muted', await T('You have {ver}. The update takes a few seconds, and the tunnel keeps running.', { ver: u.have })), notes, row]);
}

// Update the router's sing-box, or later (RouterCoreDialog)
async function openRouterCore() {
  const r = await invoke('rcore_info');
  if (!r) return;
  if (!$('panel').hidden) { setTimeout(openRouterCore, 5000); return; }   // after the dialog that is open
  const card = el('div', 'card');
  const g = el('div', 'grid');
  r.rows.forEach(([k, v], i) => { g.append(el('div', 'k', k)); const x = el('div', 'v', v); if (i === 1) x.style.fontWeight = 'bold'; g.append(x); });
  card.append(g);
  const row = el('div', 'foot-buttons');
  const install = el('button', 'accent', await T('Update router'));
  let answered = false;
  install.onclick = () => { answered = true; panel(null); act('router_core_install'); };
  const later = el('button', '', await T('Later'));
  later.onclick = () => panel(null);
  const links = el('div', 'links');
  const a = el('a', '', await T('How it is built'));
  a.onclick = () => invoke('open_url', { url: `https://github.com/${r.repo}/tree/main/router-core` });
  links.append(a);
  row.append(install, later, el('span', 'grow'), links);
  panelClosed = () => { if (!answered) invoke('rcore_later'); };
  panel([el('div', 'title', await T("Update the router's sing-box?")), card,
    el('div', 'note', await T('A smaller sing-box made for routers: only what Reality Hop uses, built from the official source. The tunnel on the router drops for about 10 seconds. If the new build does not start, the old one goes back by itself.')), row]);
}

// The questions asked once when the window opens, or the uninstall a second start asked for (realityhop.pyw start-up)
async function startAsks() {
  if (location.hash === '#uninstall') {
    history.replaceState(null, '', location.pathname);
    while (!snap) await new Promise(r => setTimeout(r, 100));
    return uninstall();
  }
  for (const q of await invoke('start_asks')) {
    const yes = await yesNo('Reality Hop', q.text);
    try {
      const line = await invoke('start_answer', { key: q.key, yes });
      if (line) await info(line);
    } catch (e) {
      await info(String(e));
      if (q.key !== 'move') return invoke('quit'); // a failed install ends this copy, as 1.7.x did
    }
  }
}

window.addEventListener('DOMContentLoaded', async () => {
  $('updateBtn').onclick = openUpdate;
  await listen('offer', e => e.payload === 'update' ? openUpdate() : openRouterCore());
  // a second start with --uninstall while this page is open changes only the #part: no reload
  window.addEventListener('hashchange', () => { if (location.hash === '#uninstall') startAsks(); });
  await startAsks();
  invoke('window_opened');
});
