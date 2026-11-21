// Router devices: the devices the router puts through the tunnel. Tick to add, untick to remove, then push. Also how to
// keep a device's address from changing, which is what the router goes by (DevicesDialog)
'use strict';
let dev = null;   // { picked, tunnelled, names, shown, all, msg, body, pushing } while the dialog is open

const ipKey = ip => ip.split('.').map(Number).reduce((a, b) => a * 256 + b, 0);

async function openDevices() {
  if (dev) return;
  dev = { picked: new Set(), tunnelled: new Set(), names: new Map(), shown: new Set(), pushing: false };
  const [allRow, all] = toggleRow(await T('Every device on the main network (this PC too), not just the ticked ones'), false);
  dev.all = all;
  all.onchange = () => fillDevices();
  const help = el('div', 'muted', await T('Ticked devices go through the tunnel on the router; everything else on your network stays as it is. Click a row to tick or untick it, then press Save and push.'));
  const card = el('div', 'card dev-list');
  const t = el('table'), cols = el('colgroup'), thead = el('thead'), hr = el('tr');
  for (const [k, h] of [['num', ''], ['ip', 'ADDRESS'], ['name', 'DEVICE'], ['state', 'STATUS']]) {
    cols.append(el('col', 'd-' + k));
    hr.append(el('th', '', h ? await T(h) : ''));
  }
  thead.append(hr);
  dev.body = el('tbody');
  t.append(cols, thead, dev.body);
  card.append(t);
  const row = el('div', 'btnrow');
  row.style.alignItems = 'center'; row.style.marginTop = '10px';
  const extra = el('input'); extra.type = 'text'; extra.style.width = '160px';
  const add = el('button', '', await T('Add'));
  add.onclick = async () => {
    const ip = extra.value.trim();
    if (!await invoke('lan_ip', { ip })) return devMsg(await T('That is not a local network address like 192.168.8.20.'), 'bad');
    dev.picked.add(ip); extra.value = ''; fillDevices();
  };
  const refresh = el('button', '', await T('Refresh list'));
  refresh.onclick = () => loadDevices(false);
  row.append(el('span', '', await T('Add an address')), extra, add, refresh);
  const info = el('div', 'card');
  info.append(el('div', 'label', await T('KEEP EACH DEVICE ON THE SAME ADDRESS')));
  const steps = ['On the phone, turn off the changing ("private" or "random") Wi-Fi address for this network, so it keeps one MAC address. iPhone: Settings → Wi-Fi → (i) next to the network → Private Wi-Fi Address: Off (or Fixed). Android: Wi-Fi → the network → Privacy → Use device MAC. Names vary by phone.',
    'Then reserve its address on the router: GL.iNet web panel > CLIENTS, then turn on the Reserved IP slider next to the device.'];
  for (let i = 0; i < steps.length; i++) info.append(el('div', 'step', `${i + 1}.  ` + await T(steps[i])));
  dev.link = el('a');
  dev.link.onclick = () => invoke('open_router_panel').catch(e => devMsg(String(e), 'bad'));
  const links = el('div', 'links'); links.append(dev.link);
  info.append(links);
  const foot = el('div', 'foot-buttons');
  dev.msg = el('span', 'muted', await T("Reading the router's device list..."));
  const save = el('button', 'accent', await T('Save and push'));
  save.onclick = saveDevices;
  const close = el('button', '', await T('Close'));
  close.onclick = () => panel(null);
  foot.append(dev.msg, el('span', 'grow'), save, close);
  panelClosed = () => { dev = null; };
  panel([el('div', 'title', await T('Router devices')), allRow, help, card, row, info, foot]);
  await loadDevices(true);
}

// Read the router's leases; the first time also what is saved (load)
async function loadDevices(first) {
  const d = await invoke('devices_get');
  if (!dev || !d) return;
  if (first) {
    dev.picked = new Set(d.picked); dev.all.checked = d.all;
    dev.link.textContent = await T('Open the GL.iNet web panel (http://{host})', { host: d.host });
  }
  dev.tunnelled = new Set(d.picked);
  dev.names = new Map(d.rows);
  dev.shown = new Set(dev.names.keys());
  await fillDevices(d.err);
}

function devMsg(text, state) {
  dev.msg.textContent = text;
  dev.msg.className = state ? 'c-' + state : 'muted';
}

// Draw the rows (fill)
async function fillDevices(err) {
  if (!dev) return;
  const ips = [...new Set([...dev.shown, ...dev.picked])].sort((a, b) => ipKey(a) - ipKey(b));
  const rows = [];
  for (const ip of ips) {
    const on = dev.picked.has(ip);
    let state = on ? await T(dev.tunnelled.has(ip) ? 'Tunnelled' : 'Added, press Save and push') : await T('Not tunnelled');
    if (on && !dev.shown.has(ip)) state += '  ·  ' + await T('Not connected now');
    const tr = el('tr', on ? 'ok' : 'unk');
    for (const x of [on ? '✓' : '', ip, dev.names.get(ip) || await T('(not in the router list)'), state]) tr.append(el('td', '', x));
    tr.onclick = () => { dev.picked.has(ip) ? dev.picked.delete(ip) : dev.picked.add(ip); fillDevices(); };
    rows.push(tr);
  }
  dev.body.replaceChildren(...rows);
  if (err) return devMsg(err, 'bad');
  devMsg(dev.all.checked ? await T('Every device on the main network goes through the tunnel.') :
    await T('{n} device(s) ticked.', { n: dev.picked.size }));
}

// Push the list to the router, after asking (save)
async function saveDevices() {
  const ips = dev.all.checked ? ['all'] : [...dev.picked].sort((a, b) => ipKey(a) - ipKey(b));
  const what = ips[0] === 'all' ? await T('Every device on the main network, this PC included') :
    ips.join('\n') || await T('(none: the router tunnel then carries no device)');
  if (!await yesNo('Router devices', await T('Push this to the router?\n\n{what}\n\nThe router tunnel restarts; it drops for a few seconds.', { what }))) return;
  devMsg(await T('Pushing to the router...'));
  dev.pushing = true;
  await invoke('devices_save', { ips });
}

// The push finished: show how it went and read the list again (push)
window.addEventListener('DOMContentLoaded', () => listen('changed', async () => {
  if (!dev || !dev.pushing) return;
  const s = await invoke('snapshot');
  if (!dev || s.busy) return;
  dev.pushing = false;
  await loadDevices(false);
  devMsg(s.status.text, s.status.state === 'ok' ? 'ok' : 'bad');
}));
