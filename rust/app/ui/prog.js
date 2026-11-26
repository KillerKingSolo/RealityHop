// The busy bars (the window's and one on top of each open dialog) fill left to right over the time the same action
// took last time (p.exp), then creep toward the end if it runs longer. p.el: seconds since the action started; p is
// null while it waits for its turn.
let progAt = null, progExp = 30, progTimer = 0, progHide = 0;
function progBars(f) { document.querySelectorAll('.prog').forEach(f); }
function progTick() {
  const f = progAt === null ? 0 : (Date.now() - progAt) / 1000 / progExp;
  const w = f < 0.9 ? f : 0.9 + 0.09 * (1 - Math.exp(-(f - 0.9) * 3));
  progBars(b => { b.hidden = false; b.style.width = (w * 100).toFixed(1) + '%'; });
}
function drawProg(busy, p) {
  if (!busy) {
    if (!progTimer) { progBars(b => { b.hidden = true; }); return; }
    clearInterval(progTimer); progTimer = 0;
    progBars(b => { b.style.width = '100%'; });
    progHide = setTimeout(() => progBars(b => { b.hidden = true; b.style.width = '0%'; }), 400);
    return;
  }
  clearTimeout(progHide);
  progAt = p ? Date.now() - p.el * 1000 : null;
  progExp = p ? Math.max(p.exp, 1) : 30;
  progTick();
  if (!progTimer) progTimer = setInterval(progTick, 200);
}
// A busy bar for the top of a dialog; it shows while an action runs
function progSlot() {
  const slot = document.createElement('div');
  slot.className = 'prog-slot';
  const bar = document.createElement('div');
  bar.className = 'prog';
  bar.hidden = !progTimer;
  slot.append(bar);
  if (progTimer) progTick();
  return slot;
}
