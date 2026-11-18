// The Reality Hop window: the tray state, the nodes in use and the watcher's actions. No library, no network: it only
// calls Reality Hop's own commands.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Put a text into an element, as text, never as markup
function put(id, text) {
  document.getElementById(id).textContent = text;
}

// Show the tray state: the pill, the ring and the tooltip
function showState(t) {
  const pill = document.getElementById('state');
  pill.className = 'pill ' + (['ok', 'warn', 'bad'].includes(t.state) ? t.state : 'unk');
  const parts = (t.tip || '').split('  ·  ');
  pill.textContent = parts[1] || pill.textContent;
  put('tip', parts.slice(2).join('  ·  '));
  const ring = document.getElementById('ring');
  ring.replaceChildren(...(t.segs.length ? t.segs : Array(6).fill('unk')).map(s => {
    const e = document.createElement('span');
    e.className = s;
    return e;
  }));
}

// Fill the node table
async function showNodes() {
  const body = document.getElementById('nodes');
  const err = document.getElementById('error');
  try {
    const nodes = await invoke('nodes');
    body.replaceChildren(...nodes.map(n => {
      const tr = document.createElement('tr');
      for (const k of ['tag', 'server', 'sni', 'fp']) {
        const td = document.createElement('td');
        td.textContent = n[k] ?? '';
        tr.append(td);
      }
      return tr;
    }));
    err.hidden = true;
  } catch (e) {
    err.textContent = String(e);
    err.hidden = false;
  }
}

// Translate the fixed texts into the person's language
async function translate() {
  for (const e of document.querySelectorAll('[data-t]')) {
    e.textContent = await invoke('text', { s: e.dataset.t });
  }
}

window.addEventListener('DOMContentLoaded', async () => {
  for (const b of document.querySelectorAll('[data-ask]')) {
    b.addEventListener('click', () => invoke('ask', { what: b.dataset.ask }));
  }
  await translate();
  put('version', 'v' + await invoke('version'));
  showState(await invoke('tray_state'));
  await showNodes();
  await listen('tray', e => { showState(e.payload); showNodes(); });
});
