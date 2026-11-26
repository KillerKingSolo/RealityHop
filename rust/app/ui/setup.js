// The setup wizard (the port of realityhop.pyw's Wizard, pick_language and ask_france). Rust runs the actions; this
// page draws the steps. No library, no network: every text goes in as text, never as markup.
'use strict';
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = id => document.getElementById(id);
const GATES = new Set(['install', 'ssh', 'box', 'devices']);   // pages whose Next waits for their action to succeed
const tcache = new Map();
let info = null, page = null, view = { ok: [], note: null, rows: [], busy: false, relaunch: false }, logw = null, picked = new Set();
let form = {};   // the inputs of the page shown

async function T(s, kw) {
  const key = kw ? s + '\u0000' + JSON.stringify(kw) : s;
  if (!tcache.has(key)) tcache.set(key, await invoke('text', { s, kw: kw || null }));
  return tcache.get(key);
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

// A question with buttons, and an optional text field (messagebox, filedialog)
function ask(title, text, buttons, input) {
  return new Promise(async resolve => {
    $('modalTitle').textContent = title;
    $('modalText').textContent = text;
    const box = $('modalInput');
    box.replaceChildren();
    let field = null;
    if (input !== undefined) { field = el('input'); field.type = 'text'; field.value = input; field.style.width = '100%'; box.append(field); }
    const b = $('modalButtons');
    b.replaceChildren();
    for (const [label, value, accent] of buttons) {
      const x = el('button', accent ? 'accent' : '', await T(label));
      x.onclick = () => { $('modal').hidden = true; resolve(field && value ? field.value : value); };
      b.append(x);
    }
    $('modal').hidden = false;
    if (field) field.focus();
  });
}

const body = () => $('body');
const seq = () => info.steps.map(s => s[0]).filter(k => info.use_router || !['ssh', 'box', 'devices'].includes(k));
const ok = gate => view.ok.includes(gate);

// ---- building blocks
const h = async text => body().append(el('h1', 'wz-h', text));
function p(text, muted, parent, bold) {
  const d = el('div', 'wz-p' + (muted ? ' muted' : '') + (bold ? ' bold' : ''), text);
  (parent || body()).append(d);
  return d;
}
function items(lines, numbered, parent) {
  for (let i = 0; i < lines.length; i++) {
    const row = el('div', 'wz-item');
    row.append(el('span', 'n', numbered ? `${i + 1}.` : '•'), el('span', '', lines[i]));
    (parent || body()).append(row);
  }
}
async function buttons(specs, parent) {
  const row = el('div', 'btnrow');
  for (const [text, fn, accent] of specs) {
    const b = el('button', accent ? 'accent' : '', text);
    b.onclick = fn;
    row.append(b);
  }
  (parent || body()).append(row);
  return row;
}
function toggle(text, on) {
  const l = el('label', 'toggle');
  const i = el('input'); i.type = 'checkbox'; i.checked = !!on;
  l.append(i, el('span', 'sw'), el('span', '', text));
  body().append(l);
  return i;
}
function note() {
  let n = body().querySelector('.wz-note');
  if (!n) { n = el('div', 'wz-note'); body().append(n); }
  return n;
}
function setNote(text, state) {
  const n = note();
  n.textContent = text || '';
  n.className = 'wz-note' + (state && state !== 'unk' ? ' c-' + state : ' muted');
}
function logBox() {
  const c = el('div', 'card wz-log');
  logw = el('pre');
  c.append(logw);
  body().append(c);
}
function link(text, url, parent) {
  const a = el('a', '', text);
  a.onclick = () => invoke('open_url', { url });
  parent.append(a);
  return a;
}
function act(name, args) {
  return invoke('wz_act', { name, args: args || null });
}

// ---- navigation (nav, show, next, back)
async function nav() {
  const busy = view.busy;
  drawProg(busy, view.prog);
  $('nextBtn').disabled = busy || (GATES.has(page) && !ok(page)) || page === 'router' || page === 'lang' || page === 'france';
  $('backBtn').disabled = busy || page === seq()[0] || page === 'lang' || page === 'france';
  $('nextBtn').textContent = await T(page === 'finish' ? 'Finish' : 'Next');
  const s = seq(), here = s.indexOf(page);
  for (const [k] of info.steps) {
    const lab = $('step-' + k);
    lab.className = 'wz-step' + (!s.includes(k) ? ' skip' : k === page ? ' here' : s.indexOf(k) < here ? ' done' : '');
  }
}

async function show(pg) {
  page = pg;
  body().replaceChildren();
  logw = null; form = {};
  await PAGES[pg]();
  if (view.note) setNote(view.note[0], view.note[1]);
  await nav();
}

async function next() {
  if (page === 'install' && view.relaunch) return invoke('wz_relaunch');   // go on in the installed copy
  if (page === 'finish') return act('wz_finish', { bg: form.bg.checked, boot: form.boot ? form.boot.checked : false, upd: form.upd.checked, files: form.files.checked });
  const s = seq();
  await clearNote();
  show(s[s.indexOf(page) + 1]);
}
async function back() {
  const s = seq();
  await clearNote();
  show(s[Math.max(0, s.indexOf(page) - 1)]);
}
async function clearNote() { view.note = null; await invoke('wz_note_clear'); }

// ---- pages
const PAGES = {
  // The language, asked once at the first start; the system's display language is marked (pick_language)
  async lang() {
    body().append(el('div', 'wz-lang-dot', '●'));
    body().append(el('div', 'wz-lang-title', 'Language  ·  Idioma  ·  语言  ·  Язык  ·  Мова\nزبان  ·  اللغة  ·  Dil  ·  Ngôn ngữ'));
    const col = el('div', 'wz-langs');
    for (const [code, name] of info.langs) {
      const b = el('button', code === info.sys_lang ? 'accent' : '', name);
      b.onclick = async () => { await invoke('wz_lang', { code }); location.reload(); };
      col.append(b);
    }
    body().append(col);
  },
  // In France, Cloudflare instead of Quad9 as the name server (ask_france)
  async france() {
    await h(await T('Do you live in France?'));
    p(await T('Reality Hop looks up website names with Quad9 (9.9.9.9). After a Paris court order, Quad9 stops serving France on 24 November 2026. In France, Reality Hop uses Cloudflare (1.1.1.2) instead.'), true);
    const pick = async dns => { await invoke('wz_france', { dns }); info.ask_france = false; show(info.start); };
    await buttons([[await T('No, keep Quad9'), () => pick('quad9'), true], [await T('Yes, use Cloudflare'), () => pick('cloudflare')]]);
  },
  async welcome() {
    await h(await T('Welcome to Reality Hop'));
    p(await T('Reality Hop hides your VPN inside a disguised connection. Networks that block VPNs (school, work, hotel, dorm and public Wi-Fi, routers with "VPN blocking", and countries that filter the internet, such as China, Russia or Iran) see only ordinary HTTPS to a big website such as cloudflare.com, so your VPN connects anyway.'));
    p(await T('It runs sing-box with VLESS + REALITY, takes free public nodes from the morpheusadam and FastNodes lists, tests them from this PC, keeps the 6 fastest that work and replaces dead ones on its own.'));
    const row = el('div', 'wz-links');
    for (const [label, text, url] of [['Nodes:', 'github.com/morpheusadam/v2ray-config', 'https://github.com/morpheusadam/v2ray-config'],
      ['', 'github.com/rtwo2/FastNodes', 'https://github.com/rtwo2/FastNodes'],
      ['Tunnel engine:', 'github.com/SagerNet/sing-box', 'https://github.com/SagerNet/sing-box']]) {
      if (label) row.append(el('span', 'muted', await T(label)));
      link(text, url, row);
    }
    body().append(row);
    p(await T('What you need'), false, null, true);
    items([await T(info.mac ? 'macOS 15 or newer, Intel or Apple silicon.' : 'Windows 10 or 11, 64-bit.'),
      await T('A VPN app that can use a SOCKS5 proxy. Private Internet Access (PIA) works out of the box; the VPN step shows how.'),
      await T('Optional: a GL.iNet router (firmware 4.x) to put other devices, such as a phone or a console, through the tunnel too.')]);
    p(await T('Public nodes are run by strangers. Your VPN still encrypts everything end to end, so a node only sees encrypted VPN traffic. Always keep the VPN on top. Make sure you are allowed to use a VPN on the network you are on, and in the country you are in.'), true);
    const star = el('div', 'card star');
    star.append(el('div', 'star-text', await T('If Reality Hop helps you, please give it a star. It is free and takes one click. Stars show me that people are using it, help others find it, and keep me motivated to fix problems and add new features.')));
    link('★  ' + await T('Star Reality Hop on GitHub'), `https://github.com/${info.repo}`, star);
    body().append(star);
  },
  async install() {
    await h(await T('Install'));
    p(await T('This step:'));
    items([...(info.frozen && !info.mac ? [await T('Copies Reality Hop into its install folder and adds it to the Start menu (one admin prompt). Everything it keeps stays in that folder (settings, nodes, logs, the tunnel engine); only your Windows account can open it, and Reality Hop updates itself there without asking again')] : []),
      await T('Downloads sing-box from its official releases (github.com/SagerNet/sing-box, about 35 MB) and checks its SHA-256 checksum'),
      await T('Downloads the public node lists (github.com/morpheusadam/v2ray-config, github.com/rtwo2/FastNodes), tests the {top} fastest candidates from this PC and keeps the 6 fastest that work (about a minute)', { top: info.top }),
      await T('Starts the tunnel on 127.0.0.1:2080')]);
    if (info.frozen && info.first_install) {   // first install: the folder can be chosen
      const row = el('div', 'btnrow');
      row.style.alignItems = 'center';
      form.folder = el('input'); form.folder.type = 'text'; form.folder.value = info.install_dir; form.folder.style.width = '420px';
      const change = el('button', '', await T('Change...'));
      change.onclick = async () => {
        const f = await ask(await T('Install Reality Hop in this folder'), '', [['Cancel', null], ['OK', true, true]], form.folder.value);
        if (f) form.folder.value = f;
      };
      row.append(el('span', '', await T('Install folder')), form.folder, change);
      body().append(row);
    } else if (info.frozen) {
      p(await T('Installed in {dir}.', { dir: info.install_dir }), true);
    }
    form.beta = toggle(await T('Use sing-box test builds (beta, alpha) instead of stable releases. Either way it updates itself daily.'), info.beta);
    if (info.frozen && !info.mac) {
      form.menu = toggle(await T('Show Reality Hop in the Start menu and Windows search (press the Windows key, type Reality Hop)'), info.start_menu);
      form.desk = toggle(await T('Desktop icon'), info.desktop);
    }
    await buttons([[await T(ok('install') ? 'Install again (repair)' : 'Install'), () => act('wz_install', {
      testing: form.beta.checked, menu: form.menu ? form.menu.checked : info.start_menu, desk: form.desk ? form.desk.checked : info.desktop,
      folder: form.folder ? form.folder.value : null }), true]]);
    if (ok('install') && !view.note) setNote(await T('Already installed. Press Next, or Install again to repair.'), 'ok');
    logBox();
  },
  async router() {
    await h(await T('Do you have a GL.iNet router?'));
    p(await T('With the router set up, devices you choose (a phone, a games console, a TV) also go through the tunnel, with nothing to install on them. Only their traffic is diverted; everything else on your network stays as it is.'));
    p(await T("Tested on a GL-AXT1800 with firmware 4.x and the router's sing-box 1.9. Other GL.iNet routers on firmware 4.x use the same system and should work the same way. You can turn this on or off later in Settings."), true);
    p(await T("Router only: nothing runs on this PC's connection. The router tunnels the devices you choose, or every device on its main network, this PC included. For people who want all their traffic to go through the tunnel."), true);
    const pick = async mode => {   // 'both', 'router' (router only) or 'pc'
      await invoke('wz_mode', { mode });
      info.use_router = mode !== 'pc';
      info.ronly = mode === 'router';
      const s = seq();
      show(s[s.indexOf('router') + 1]);
    };
    await buttons([[await T('Yes, this PC and my router'), () => pick('both'), true], [await T('Router only'), () => pick('router')],
      [await T('No, only this PC'), () => pick('pc')]]);
  },
  async ssh() {
    await h(await T('Router step 1 of 3: connect'));
    p(await T('Reality Hop talks to the router over SSH with its own key, so it never needs or keeps your router password. SSH is on by default on GL.iNet routers.'));
    const row = el('div', 'btnrow');
    row.style.alignItems = 'center';
    form.host = el('input'); form.host.type = 'text'; form.host.value = info.host; form.host.style.width = '150px';
    row.append(el('span', '', await T('Router address')), form.host, el('span', 'muted', await T('(GL.iNet default: 192.168.8.1)')));
    body().append(row);
    items([await T('Click "Add key to router". A black window opens and asks for the router password: type the admin password of the GL.iNet web panel and press Enter (nothing shows while you type). When it says "Key added", close it.'),
      await T('Click "Test connection".')], true);
    await buttons([
      [await T('Add key to router'), async () => {
        try { setNote(await invoke('wz_add_key', { host: form.host.value })); } catch (e) { setNote(await T('Something failed: {e}', { e: String(e) }), 'bad'); }
      }],
      [await T('Test connection'), () => act('wz_test_ssh', { host: form.host.value }), true],
      [await T('Use my own key file...'), async () => {
        const f = await ask(await T('Private key that the router already accepts'), '', [['Cancel', null], ['OK', true, true]], '');
        if (!f) return;
        try { setNote(await invoke('wz_own_key', { path: f })); } catch (e) { setNote(String(e), 'bad'); }
      }]]);
    if (ok('ssh') && !view.note) setNote(await T('Connected.'), 'ok');
  },
  async box() {
    await h(await T('Router step 2 of 3: sing-box on the router'));
    p(await T("The router runs its own copy of sing-box for the devices you choose. Reality Hop can install it from the router's package list, or you can do it yourself: GL.iNet web panel → APPLICATIONS → Plug-ins → Update, then search for \"sing-box\" and install it, and also \"jq\" and \"curl\" (used by the router's self-repair)."));
    await buttons([[await T('Check again'), () => act('wz_check_box')], [await T('Install sing-box on the router'), () => act('wz_box_install'), true]]);
    logBox();
    act('wz_check_box');
  },
  async devices() {
    await h(await T('Router step 3 of 3: choose devices'));
    p(await T("Click the devices whose traffic should go through the tunnel. The list is the router's DHCP clients (devices seen recently)."));
    form.all = toggle(await T('Every device on the main network (this PC too): all traffic goes through the tunnel'), info.all_dev);
    const c = el('div', 'card dev-list');
    const t = el('table'), cols = el('colgroup'), thead = el('thead'), hr = el('tr');
    for (const [k, hd] of [['num', ''], ['ip', 'ADDRESS'], ['name', 'DEVICE']]) { cols.append(el('col', 'd-' + k)); hr.append(el('th', '', hd ? await T(hd) : '')); }
    thead.append(hr);
    form.rows = el('tbody');
    t.append(cols, thead, form.rows);
    c.append(t);
    body().append(c);
    const row = el('div', 'btnrow');
    row.style.alignItems = 'center';
    form.extra = el('input'); form.extra.type = 'text'; form.extra.style.width = '320px';
    row.append(el('span', '', await T('Other addresses')), form.extra, el('span', 'muted', await T('Comma separated')));
    body().append(row);
    p(await T('Give each chosen device a fixed address, or it can lose the tunnel when its address changes. On phones, first turn off rotating private addresses for this Wi-Fi (iPhone: Private Wi-Fi Address set to Off or Fixed; Android: use device MAC). Then GL.iNet web panel > CLIENTS: turn on the Reserved IP slider next to the device.'), true);
    await buttons([[await T('Push to router'), () => act('wz_push_devices', { extra: form.extra.value, every: form.all.checked, picked: [...picked] }), true]]);
    logBox();
    for (const ip of info.devices) picked.add(ip);
    form.filled = false;
    act('wz_load_devices');
  },
  async vpn() {
    if (info.ronly) {
      await h(await T('Your VPN app'));
      p(await T('In router-only mode your VPN app needs no proxy setting: the router puts the traffic of the chosen devices through the tunnel. Connect the VPN app as usual. OpenVPN over TCP port 443 is the most reliable through the tunnel.'));
      p(await T('If PIA on this PC still uses the SOCKS5 proxy 127.0.0.1:2080, turn it off: Settings → Multi-Hop, untick "Multi-hop and Obfuscation".'), true);
      return;
    }
    await h(await T('Point your VPN app at the tunnel'));
    p(await T("Your VPN app connects through Reality Hop's SOCKS5 proxy: address 127.0.0.1, port 2080, no username or password. Use OpenVPN over TCP port 443: a SOCKS5 proxy carries TCP only, and 443 is the normal HTTPS port."));
    const cols = el('div', 'wz-cols'), left = el('div', 'wz-left'), right = el('div', 'wz-right');
    cols.append(left, right);
    body().append(cols);
    p(await T('Private Internet Access (PIA)'), false, left, true);
    items([await T('PIA → Settings → Protocols: Protocol OpenVPN, Transport TCP, Remote Port 443.'),
      await T('Settings → Multi-Hop: tick "Multi-hop and Obfuscation" and choose SOCKS5 Proxy.'),
      await T('Click EDIT: host 127.0.0.1, port 2080. Leave username and password empty. Save.'),
      ...(info.split ? [info.split] : []), await T('Connect.')], true, left);
    await buttons([[await T('Check PIA'), async () => { const [t, s] = await invoke('wz_check_pia'); setNote(t, s); }]], left);
    p(await T('Other VPN apps'), false, left, true);
    p(await T('Any VPN app with a SOCKS5 proxy setting works the same way; set it to OpenVPN over TCP, port 443. In an OpenVPN config file: "proto tcp", the server on port 443, and the line "socks-proxy 127.0.0.1 2080".'), false, left);
    p(await T("Games or apps that should skip the VPN: use your VPN app's split tunnel (PIA: Settings → Split Tunnel)."), true, left);
    for (const name of ['pia-multihop.png', 'pia-split-tunnel.png']) {
      const img = el('img'); img.src = 'img/' + name; img.alt = '';
      right.append(img);
    }
  },
  async finish() {
    await h(await T('Almost done'));
    form.bg = toggle(await T('Run Reality Hop in the background at logon (tray icon; it repairs dead nodes on its own)'), true);
    if (!info.ronly) form.boot = toggle(await T('Start the tunnel at boot, before the VPN app (asks for admin rights once)'), true);
    form.upd = toggle(await T('Install Reality Hop updates automatically'), info.auto_update);
    form.files = toggle(await T('Show backend files: buttons for the PC, Router and logs folders, to see what runs behind the scenes'), info.backend_files);
    p(await T('You can change all of this later in the Reality Hop window. Press Finish to open it.'), true);
    logBox();
  },
};

// The router's clients: a row per device, a tick on the chosen ones (load_devices, toggle_dev)
function fillDevices() {
  const shown = new Set(view.rows.map(r => r[0]));
  form.rows.replaceChildren(...view.rows.map(([ip, name]) => {
    const tr = el('tr');
    const tick = el('td', '', picked.has(ip) ? '✓' : '');
    tr.append(tick, el('td', '', ip), el('td', '', name));
    tr.onclick = () => { picked.has(ip) ? picked.delete(ip) : picked.add(ip); tick.textContent = picked.has(ip) ? '✓' : ''; };
    return tr;
  }));
  form.extra.value = [...picked].filter(ip => !shown.has(ip)).sort().join(', ');
  form.filled = true;
}

// A change in Rust: the note, the gates, the busy bar; the device list once read; Finish opens the window
async function refresh() {
  view = await invoke('wz_view');
  if (page === 'finish' && view.ok.includes('finish')) { location.href = 'index.html'; return; }
  if (view.note) setNote(view.note[0], view.note[1]);
  if (page === 'devices' && form.rows && !form.filled && view.rows.length) fillDevices();
  await nav();
}

function logLine(at, line) {
  if (!logw) return;
  logw.append(el('span', 't', at + '  '), el('span', /^(Error|FAIL)/.test(line) || line.includes('NOT ') ? 'err' : '', line + '\n'));
  logw.parentElement.scrollTop = logw.parentElement.scrollHeight;
}

window.addEventListener('DOMContentLoaded', async () => {
  const hash = location.hash.slice(1);
  info = await invoke('wz_info', { routerOnly: hash === 'ronly' });
  if (hash === 'router') info.start = 'router';
  document.title = await T('Reality Hop setup');
  $('brandName').textContent = await T(' Reality Hop');
  $('setupLine').textContent = await T('Setup') + '  ·  ' + info.version;
  $('steps').replaceChildren(...info.steps.map(([k, name]) => { const d = el('div', 'wz-step', name); d.id = 'step-' + k; return d; }));
  $('nextBtn').onclick = next;
  $('backBtn').onclick = back;
  $('backBtn').textContent = await T('Back');
  await listen('log', e => logLine(e.payload.at, e.payload.line));
  await listen('changed', refresh);
  view = await invoke('wz_view');
  await show(info.ask_lang ? 'lang' : info.ask_france ? 'france' : info.start);
});
